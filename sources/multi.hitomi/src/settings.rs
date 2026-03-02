use aidoku::{alloc::String, imports::defaults::defaults_get};

const LANGUAGE_KEY: &str = "language";
const TITLE_PREFERENCE_KEY: &str = "titlePreference";

pub fn get_nozomi_language() -> String {
	defaults_get::<String>(LANGUAGE_KEY)
		.map(|lang| match lang.as_str() {
			"en" => "english".into(),
			"ja" => "japanese".into(),
			"zh" => "chinese".into(),
			"ko" => "korean".into(),
			"fr" => "french".into(),
			"de" => "german".into(),
			"es" => "spanish".into(),
			"ru" => "russian".into(),
			"id" => "indonesian".into(),
			"vi" => "vietnamese".into(),
			"th" => "thai".into(),
			"ar" => "arabic".into(),
			"pl" => "polish".into(),
			"pt" => "portuguese".into(),
			"hu" => "hungarian".into(),
			"it" => "italian".into(),
			"cs" => "czech".into(),
			"nl" => "dutch".into(),
			"fi" => "finnish".into(),
			"sv" => "swedish".into(),
			"tr" => "turkish".into(),
			"uk" => "ukrainian".into(),
			_ => "all".into(),
		})
		.unwrap_or_else(|| "all".into())
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TitlePreference {
	#[default]
	English,
	Japanese,
}

impl From<String> for TitlePreference {
	fn from(value: String) -> Self {
		match value.as_str() {
			"japanese" => Self::Japanese,
			_ => Self::English,
		}
	}
}

pub fn get_title_preference() -> TitlePreference {
	defaults_get::<String>(TITLE_PREFERENCE_KEY)
		.map(TitlePreference::from)
		.unwrap_or_default()
}
