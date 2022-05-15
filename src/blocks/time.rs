use super::prelude::*;
use chrono::offset::{Local, Utc};
use chrono::Locale;
use chrono_tz::Tz;

pub struct Time {
    text: TextWidget,
    update_interval: Duration,
    formats: (String, Option<String>),
    timezone: Option<Tz>,
    locale: Option<String>,
}

#[derive(Deserialize, Debug, SmartDefault)]
#[serde(deny_unknown_fields, default)]
pub struct TimeConfig {
    format: FormatTemplate,
    #[default(5.into())]
    interval: Seconds,
    timezone: Option<Tz>,
    locale: Option<String>,
}

#[async_trait]
impl ConfigBlock for Time {
    type Config = TimeConfig;

    async fn new(
        id: usize,
        config: Self::Config,
        shared_config: SharedConfig,
        _: Sender<usize>,
    ) -> Result<Self> {
        Ok(Time {
            text: TextWidget::new(id, 0, shared_config)
                .with_text("")
                .with_icon("time")?,
            update_interval: config.interval.0,
            formats: config
                .format
                .with_default("%a %d/%m %R")?
                .render(&HashMap::<&str, _>::new())?,
            timezone: config.timezone,
            locale: config.locale,
        })
    }
}

impl Time {
    fn get_formatted_time(&self, format: &str) -> Result<String> {
        let time = match &self.locale {
            Some(l) => {
                let locale: Locale = l.as_str().try_into().ok().error_msg("invalid locale")?;
                match self.timezone {
                    Some(tz) => Utc::now()
                        .with_timezone(&tz)
                        .format_localized(format, locale),
                    None => Local::now().format_localized(format, locale),
                }
            }
            None => match self.timezone {
                Some(tz) => Utc::now().with_timezone(&tz).format(format),
                None => Local::now().format(format),
            },
        };
        Ok(format!("{}", time))
    }
}

#[async_trait]
impl Block for Time {
    fn interval(&self) -> Option<Duration> {
        Some(self.update_interval)
    }

    async fn update(&mut self) -> Result<()> {
        if self.timezone.is_none() {
            // Update timezone because `chrono` will not do that for us.
            // https://github.com/chronotope/chrono/issues/272
            unsafe { tzset() };
        }

        let full = self.get_formatted_time(&self.formats.0)?;
        let short = match &self.formats.1 {
            Some(short_fmt) => Some(self.get_formatted_time(short_fmt)?),
            None => None,
        };

        self.text.set_texts((full, short));
        Ok(())
    }

    fn view(&self) -> Vec<&dyn I3BarWidget> {
        vec![&self.text]
    }
}

extern "C" {
    /// The tzset function initializes the tzname variable from the value of the TZ environment
    /// variable. It is not usually necessary for your program to call this function, because it is
    /// called automatically when you use the other time conversion functions that depend on the
    /// time zone.
    fn tzset();
}
