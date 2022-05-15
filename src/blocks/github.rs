use super::prelude::*;

const GITHUB_TOKEN_ENV: &str = "I3RS_GITHUB_TOKEN";

pub struct Github {
    text: TextWidget,
    hidden: bool,
    token: String,
    update_interval: Duration,
    api_server: String,
    format: FormatTemplate,
    hide_if_total_is_zero: bool,
    good: Option<Vec<String>>,
    info: Option<Vec<String>>,
    warning: Option<Vec<String>>,
    critical: Option<Vec<String>>,
}

#[derive(Deserialize, Debug, Clone, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct GithubConfig {
    #[default(30.into())]
    interval: Seconds,
    #[default("https://api.github.com".into())]
    api_server: String,
    format: FormatTemplate,
    hide_if_total_is_zero: bool,
    good: Option<Vec<String>>,
    info: Option<Vec<String>>,
    warning: Option<Vec<String>>,
    critical: Option<Vec<String>>,
}

#[async_trait]
impl ConfigBlock for Github {
    type Config = GithubConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(Self {
            text: TextWidget::new(id, 0, shared_config)
                .with_text("x")
                .with_icon("github")?,
            hidden: true,
            token: std::env::var(GITHUB_TOKEN_ENV)
                .error_msg("missing I3RS_GITHUB_TOKEN environment variable")?,
            update_interval: config.interval.0,
            api_server: config.api_server,
            format: config.format.with_default("{total:1}")?,
            hide_if_total_is_zero: config.hide_if_total_is_zero,
            good: config.good,
            info: config.info,
            warning: config.warning,
            critical: config.critical,
        })
    }
}

#[async_trait]
impl Block for Github {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        let mut total = 0;
        let v = fold_notifications(
            &self.api_server,
            &self.token,
            map!("total".to_owned() => 0),
            |map, n| {
                map.entry(n.reason).and_modify(|v| *v += 1).or_insert(1);
                total += 1;
            },
        )
        .await?;
        let get = |key: &str| v.get(key).map(|x| *x).unwrap_or_default();
        let values = map!(
            "total" => Value::from_integer(total as i64),
            // As specified by:
            // https://developer.github.com/v3/activity/notifications/#notification-reasons
            "assign" =>           Value::from_integer(get("assign") as i64),
            "author" =>           Value::from_integer(get("author") as i64),
            "comment" =>          Value::from_integer(get("comment") as i64),
            "invitation" =>       Value::from_integer(get("invitation") as i64),
            "manual" =>           Value::from_integer(get("manual") as i64),
            "mention" =>          Value::from_integer(get("mention") as i64),
            "review_requested" => Value::from_integer(get("review_requested") as i64),
            "security_alert" =>   Value::from_integer(get("security_alert") as i64),
            "state_change" =>     Value::from_integer(get("state_change") as i64),
            "subscribed" =>       Value::from_integer(get("subscribed") as i64),
            "team_mention" =>     Value::from_integer(get("team_mention") as i64),
        );

        if total == 0 && self.hide_if_total_is_zero {
            self.hidden = true;
        } else {
            self.hidden = false;
            self.text.set_texts(self.format.render(&values).unwrap());
            self.text.set_state(get_state(
                &self.critical,
                &self.warning,
                &self.info,
                &self.good,
                &v,
            ));
        }

        Ok(())
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        if self.hidden {
            vec![]
        } else {
            vec![&self.text]
        }
    }
}

#[derive(Deserialize)]
struct Notification {
    reason: String,
}

// https://docs.github.com/en/rest/reference/activity#notifications
async fn get_on_page(api_server: &str, token: &str, page: usize) -> Result<Vec<Notification>> {
    REQWEST_CLIENT
        .get(format!(
            "{api_server}/notifications?per_page=100&page={page}",
        ))
        .header("Authorization", format!("token {token}"))
        .send()
        .await
        .error_msg("Failed to send request")?
        .json()
        .await
        .error_msg("Failed to get JSON")
}

async fn fold_notifications<T, F: FnMut(&mut T, Notification)>(
    api_server: &str,
    token: &str,
    mut state: T,
    mut f: F,
) -> Result<T> {
    for page in 1.. {
        let on_page = get_on_page(api_server, token, page).await?;
        if on_page.is_empty() {
            break;
        }
        for notification in on_page {
            f(&mut state, notification);
        }
    }
    Ok(state)
}

fn get_state(
    critical: &Option<Vec<String>>,
    warning: &Option<Vec<String>>,
    info: &Option<Vec<String>>,
    good: &Option<Vec<String>>,
    agg: &HashMap<String, u64>,
) -> State {
    let default: u64 = 0;
    for (list_opt, ret) in &[
        (critical, State::Critical),
        (warning, State::Warning),
        (info, State::Info),
        (good, State::Good),
    ] {
        if let Some(list) = list_opt {
            for key in agg.keys() {
                if list.contains(key) && *agg.get(key).unwrap_or(&default) > 0 {
                    return *ret;
                }
            }
        }
    }
    State::Idle
}
