use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

mod tse_date_format {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    pub struct Naive;

    const FORMAT: &'static str = "%Y %m %d %H %M %S";

    impl SerializeAs<NaiveDateTime> for Naive {
        fn serialize_as<S>(value: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let s = format!("{}", value.format(FORMAT));
            serializer.serialize_str(&s)
        }
    }

    impl<'de> DeserializeAs<'de, NaiveDateTime> for Naive {
        fn deserialize_as<D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            Ok(NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)?)
        }
    }
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct Data {
    pub roll: RollData,
    pub exposures: HashMap<u32, ExposureSpecificData>,
}

#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct RollData {
    pub author: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub iso: Option<String>,
    pub description: Option<String>,
}

#[serde_with::serde_as]
#[derive(Clone, Default, Debug, Deserialize, Serialize)]
pub struct ExposureSpecificData {
    pub sspeed: Option<String>,
    pub aperture: Option<String>,
    pub lens: Option<String>,
    pub comment: Option<String>,
    #[serde_as(as = "Option<tse_date_format::Naive>")]
    pub date: Option<NaiveDateTime>,
    pub gps: Option<(f64, f64)>,
}

impl RollData {
    pub fn update_field(&mut self, key: &str, value: String) {
        match key {
            "author" => self.author = Some(value),
            "make" => self.make = Some(value),
            "model" => self.model = Some(value),
            "iso" => self.iso = Some(value),
            "description" => self.description = Some(value),
            _ => todo!(),
        }
    }

    pub fn as_tsv(&self) -> String {
        format!(
            "#Description {}
#ImageDescription {}
#Artist {}
#Author {}
#ISO {}
#Make {}
#Model {}
; vim: set list number noexpandtab:",
            self.description.clone().unwrap_or(String::new()),
            self.description.clone().unwrap_or(String::new()),
            self.author.clone().unwrap_or(String::new()),
            self.author.clone().unwrap_or(String::new()),
            self.iso.clone().unwrap_or(String::new()),
            self.make.clone().unwrap_or(String::new()),
            self.model.clone().unwrap_or(String::new()),
        )
    }
}

impl ExposureSpecificData {
    pub fn update_field(&mut self, key: &str, value: String) {
        match key {
            "sspeed" => self.sspeed = Some(value),
            "aperture" => self.aperture = Some(value),
            "lens" => self.lens = Some(value),
            "comment" => self.comment = Some(value),
            "gps" => {
                self.gps = {
                    let split = value.split(",").collect::<Vec<_>>();
                    if split.len() == 2 {
                        match (
                            split[0].trim().parse::<f64>(),
                            split[1].trim().parse::<f64>(),
                        ) {
                            (Ok(lat), Ok(lon)) => Some((lat, lon)),
                            (Err(e), _) => {
                                log::error!("lat error: {e}");
                                None
                            }
                            (_, Err(e)) => {
                                log::error!("lon error: {e}");
                                None
                            }
                        }
                    } else {
                        log::error!("Invalid gps coordinates format !");
                        None
                    }
                }
            }
            "date" => {
                self.date = NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S")
                    .or(NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M"))
                    .ok()
            }
            _ => todo!(),
        }
    }

    fn as_tsv(&self) -> String {
        let mut fields = vec![
            self.sspeed.clone().unwrap_or(String::new()),
            self.aperture.clone().unwrap_or(String::new()),
            self.lens.clone().unwrap_or(String::new()),
            self.comment.clone().unwrap_or(String::new()),
        ];

        fields.push(
            self.date
                .map(|d| format!("{}", d.format("%Y %m %d %H %M %S")))
                .unwrap_or(String::new()),
        );

        match self.gps {
            None => fields.push(String::new()),
            Some((lat, lon)) => fields.push(format!("{lat}, {lon}")),
        }

        fields.join("\t")
    }
}

impl Data {
    pub fn to_string(&self) -> String {
        let mut lines: Vec<String> = vec![];
        let max: u32 = *self.exposures.keys().max().unwrap_or(&0u32) + 1;

        for index in 1..max {
            match self.exposures.get(&index) {
                Some(exp) => lines.push(exp.as_tsv()),
                None => lines.push(String::new()),
            }
        }

        lines.push(self.roll.as_tsv());
        lines.join("\n")
    }
}
