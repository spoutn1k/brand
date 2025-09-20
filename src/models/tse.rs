use crate::{
    Error,
    models::{Data, ExposureSpecificData, RollData},
};
use chrono::NaiveDateTime;
use winnow::{
    ModalResult, Parser as _,
    ascii::{alphanumeric1, float, tab},
    combinator::{opt, preceded, separated_pair, seq},
    stream::AsChar,
    token::{take_till, take_while},
};

pub mod date_format {
    use chrono::NaiveDateTime;
    use serde::{self, Deserialize, Deserializer, Serializer};
    use serde_with::{DeserializeAs, SerializeAs};

    pub struct Naive;

    pub const FORMAT: &str = "%Y %m %d %H %M %S";

    impl SerializeAs<NaiveDateTime> for Naive {
        fn serialize_as<S>(value: &NaiveDateTime, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let s = value.format(FORMAT).to_string();
            serializer.serialize_str(&s)
        }
    }

    impl<'de> DeserializeAs<'de, NaiveDateTime> for Naive {
        fn deserialize_as<D>(deserializer: D) -> Result<NaiveDateTime, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::deserialize(deserializer)?;
            NaiveDateTime::parse_from_str(&s, FORMAT).map_err(serde::de::Error::custom)
        }
    }
}

fn exposure_tsv(input: &mut &str) -> ModalResult<ExposureSpecificData> {
    seq! {ExposureSpecificData {
        sspeed: opt(alphanumeric1.map(String::from)),
        _: tab,
        aperture: opt(preceded(opt("f"), float).map(|m: f32| m.to_string())),
        _: tab,
        lens: opt(
            take_while(1.., |c| {AsChar::is_alphanum(c) || [' ', '-', '.'].contains(&c)})
                .map(String::from)
            ),
        _: tab,
        comment: take_till(0.., |c| c == '\t')
            .map(|m| Some(String::from(m))),
        _: tab,
        date: take_till(0.., |c| c == '\t')
            .map(|s: &str| {
                NaiveDateTime::parse_from_str(s, date_format::FORMAT).ok()
            }),
        _: tab,
        gps: opt(separated_pair(float, ", ", float)),
    }}
    .parse_next(input)
}

pub fn read_tse<R: std::io::BufRead>(buffer: R) -> Result<Data, Error> {
    let Data {
        mut roll,
        mut exposures,
    } = Data::default();

    let mut reader = buffer.lines();

    let mut index = 1;
    while let Some(line) = reader.next().transpose()? {
        if let Some(stripped) = line.strip_prefix('#') {
            let space = stripped.find(' ').unwrap();
            let (marker, value) = stripped.split_at(space);

            match marker.to_lowercase().as_str() {
                "make" => roll.make = Some(value.trim().into()),
                "model" => roll.model = Some(value.trim().into()),
                "description" => roll.description = Some(value.trim().into()),
                "author" => roll.author = Some(value.trim().into()),
                "iso" => roll.iso = Some(value.trim().into()),
                _ => (),
            }

            continue;
        }

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        let exposure = exposure_tsv
            .parse(&line)
            .map_err(|e| Error::ParseTse(index, e.to_string()))?;
        exposures.insert(index, exposure);
        index += 1;
    }

    Ok(Data { roll, exposures })
}

pub trait TseFormat {
    fn as_tse(&self) -> String;
}

impl TseFormat for ExposureSpecificData {
    fn as_tse(&self) -> String {
        let mut fields = vec![
            self.sspeed.clone().unwrap_or_default(),
            self.aperture.clone().unwrap_or_default(),
            self.lens.clone().unwrap_or_default(),
            self.comment.clone().unwrap_or_default(),
            self.date
                .map(|d| d.format("%Y %m %d %H %M %S").to_string())
                .unwrap_or_default(),
        ];

        match self.gps {
            None => fields.push(String::new()),
            Some((lat, lon)) => fields.push(format!("{lat}, {lon}")),
        }

        fields.join("\t")
    }
}

impl TseFormat for RollData {
    fn as_tse(&self) -> String {
        format!(
            "#Description {}
#ImageDescription {}
#Artist {}
#Author {}
#ISO {}
#Make {}
#Model {}
; vim: set list number noexpandtab:",
            self.description.as_deref().unwrap_or_default(),
            self.description.as_deref().unwrap_or_default(),
            self.author.as_deref().unwrap_or_default(),
            self.author.as_deref().unwrap_or_default(),
            self.iso.as_deref().unwrap_or_default(),
            self.make.as_deref().unwrap_or_default(),
            self.model.as_deref().unwrap_or_default(),
        )
    }
}

impl TseFormat for Data {
    fn as_tse(&self) -> String {
        let max: u32 = *self.exposures.keys().max().unwrap_or(&0u32) + 1;

        (1..max)
            .map(|index| {
                self.exposures
                    .get(&index)
                    .map(|exp| exp.as_tse())
                    .unwrap_or_default()
            })
            .chain(self.roll.as_tse().lines().map(String::from))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn test_read_tse() {
    let tse = r#"
	2.8	50mm	Balade dans Paris	2025 04 10 14 00 00	48.86467241, 2.32135534
	f1.8	50mm	Balade dans Paris	2025 04 10 14 00 00	48.86467241, 2.32135534
		50mm	Balade dans Paris	2025 04 10 14 00 00	48.86351491, 2.31966019
		50mm	Balade dans Paris	2025 04 10 14 00 00	48.84697524, 2.33650446
		85mm	Balade dans Paris	2025 04 10 14 00 00	48.84697524, 2.33650446
		85mm	Balade dans Paris	2025 04 10 14 00 00	48.84697524, 2.33650446
		85mm	Balade dans Paris	2025 04 10 14 00 00	48.84697524, 2.33650446
		85mm	Balade dans Paris	2025 04 10 14 00 00	48.8472506, 2.34434724
		50mm	Balade dans Paris	2025 04 10 14 00 00	48.8472506, 2.34434724
		85mm	Balade dans Paris	2025 04 10 14 00 00	48.85359036, 2.34680414
		50mm	Tourisme	2025 04 15 16 00 00	48.86167979, 2.29603529
		50mm	Tourisme	2025 04 15 16 00 00	48.86167979, 2.29603529
		50mm	Tourisme	2025 04 15 16 00 00	48.86167979, 2.29603529
		50mm	Tourisme	2025 04 15 16 00 00	48.86167979, 2.29603529
		20mm	Ile de la cité	2025 04 17 18 00 00	48.85519988, 2.34651446
		20mm	Ile de la cité	2025 04 17 18 00 00	48.85519988, 2.34651446
		85mm	Balade	2025 04 18 17 00 00	48.87177211, 2.36459255
		85mm	Balade	2025 04 18 17 00 00	48.87177211, 2.36459255
		85mm	Balade	2025 04 18 17 00 00	48.87177211, 2.36459255
		85mm	Balade	2025 04 18 17 00 00	48.87177211, 2.36459255
			Scenes	2025 04 20 16 00 00	48.88082524, 2.36210346
		50mm	Scenes	2025 04 21 18 00 00	48.85385155, 2.36964583
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Hasard ludique	2025 04 26 17 00 00	48.89575261, 2.32966483
		50mm	Repu	2025 04 27 17 00 00	48.86817299, 2.36290812
#Description Kodak Portra
#ImageDescription Kodak Portra
#Artist Jean-Baptiste Skutnik
#Author Jean-Baptiste Skutnik
#ISO 160
#Make Nikon
#Model F3
; vim: set list number noexpandtab:
"#;
    let parsed = read_tse(tse.as_bytes()).unwrap();

    insta::assert_debug_snapshot!(parsed);
    insta::assert_snapshot!(parsed.as_tse())
}
