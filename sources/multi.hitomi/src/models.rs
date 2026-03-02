use aidoku::{
	ContentRating, Manga, MangaStatus, UpdateStrategy, Viewer,
	alloc::{String, Vec},
	prelude::*,
};
use serde::Deserialize;

use crate::settings::{TitlePreference, get_title_preference};

#[derive(Deserialize, Clone)]
pub struct HitomiTag {
	pub tag: String,
	#[serde(default)]
	pub female: String,
	#[serde(default)]
	pub male: String,
}

#[derive(Deserialize, Clone)]
pub struct HitomiArtist {
	pub artist: String,
}

#[derive(Deserialize, Clone)]
pub struct HitomiGroup {
	pub group: String,
}

#[derive(Deserialize, Clone)]
pub struct HitomiParody {
	pub parody: String,
}

#[derive(Deserialize, Clone)]
pub struct HitomiCharacter {
	pub character: String,
}

#[derive(Deserialize, Clone)]
pub struct HitomiFile {
	pub hash: String,
	#[serde(default)]
	pub haswebp: u8,
	#[serde(default)]
	pub hasavif: u8,
	#[serde(default)]
	pub hasjxl: u8,
	#[serde(default)]
	pub width: u32,
	#[serde(default)]
	pub height: u32,
}

impl HitomiFile {
	pub fn is_gif(&self) -> bool {
		self.haswebp == 1 && self.hasavif == 0
	}
}

#[derive(Deserialize, Clone)]
pub struct HitomiLanguage {
	pub galleryid: String,
	pub name: String,
	#[serde(default)]
	pub url: String,
}

#[derive(Deserialize, Clone)]
pub struct HitomiGallery {
	pub id: String,
	pub title: String,
	#[serde(rename = "japanese_title")]
	pub japanese_title: Option<String>,
	#[serde(rename = "galleryurl")]
	pub gallery_url: String,
	pub r#type: String,
	#[serde(default)]
	pub language: Option<String>,
	pub date: String,
	pub files: Vec<HitomiFile>,
	#[serde(default)]
	pub artists: Option<Vec<HitomiArtist>>,
	#[serde(default)]
	pub groups: Option<Vec<HitomiGroup>>,
	#[serde(default)]
	pub parodys: Option<Vec<HitomiParody>>,
	#[serde(default)]
	pub characters: Option<Vec<HitomiCharacter>>,
	#[serde(default)]
	pub tags: Option<Vec<HitomiTag>>,
	#[serde(default)]
	pub languages: Option<Vec<HitomiLanguage>>,
	#[serde(default)]
	pub related: Option<Vec<i64>>,
}

impl HitomiGallery {
	pub fn cover_url(&self) -> String {
		let hash = self.files.first().map(|f| f.hash.as_str()).unwrap_or("");
		let len = hash.len();
		if len < 3 {
			return String::new();
		}
		let last1 = &hash[len - 1..];
		let last2 = &hash[len - 3..len - 1];
		format!("https://atn.gold-usergeneratedcontent.net/avifbigtn/{last1}/{last2}/{hash}.avif")
	}
}

impl From<HitomiGallery> for Manga {
	fn from(g: HitomiGallery) -> Self {
		let artists: Vec<String> = g
			.artists
			.as_deref()
			.unwrap_or_default()
			.iter()
			.map(|a| a.artist.clone())
			.collect();

		let groups: Vec<String> = g
			.groups
			.as_deref()
			.unwrap_or_default()
			.iter()
			.map(|gr| gr.group.clone())
			.collect();

		// authors = groups + artists (combined), matching nhentai pattern
		let combined_authors: Vec<String> = [groups.clone(), artists.clone()].concat();

		let mut tags: Vec<String> = Vec::new();
		if let Some(tag_list) = &g.tags {
			for t in tag_list {
				if t.female == "1" {
					tags.push(format!("female:{}", t.tag));
				} else if t.male == "1" {
					tags.push(format!("male:{}", t.tag));
				} else {
					tags.push(t.tag.clone());
				}
			}
		}

		let title_preference = get_title_preference();

		let mut description_parts: Vec<String> = Vec::new();
		match title_preference {
			TitlePreference::Japanese => {
				if g.japanese_title.is_some() {
					description_parts.push(format!("English title: {}", g.title));
				}
			}
			TitlePreference::English => {
				if let Some(jp) = &g.japanese_title {
					description_parts.push(format!("Japanese title: {jp}"));
				}
			}
		}
		if let Some(parodys) = &g.parodys
			&& !parodys.is_empty()
		{
			let s: Vec<String> = parodys.iter().map(|p| p.parody.clone()).collect();
			description_parts.push(format!("Series: {}", s.join(", ")));
		}
		if let Some(characters) = &g.characters
			&& !characters.is_empty()
		{
			let s: Vec<String> = characters.iter().map(|c| c.character.clone()).collect();
			description_parts.push(format!("Characters: {}", s.join(", ")));
		}
		description_parts.push(format!("Type: {}", g.r#type));
		description_parts.push(format!("Pages: {}", g.files.len()));

		let viewer = if g.r#type == "anime" {
			Viewer::Vertical
		} else {
			Viewer::RightToLeft
		};

		let title = match title_preference {
			TitlePreference::Japanese => {
				g.japanese_title.clone().unwrap_or_else(|| g.title.clone())
			}
			TitlePreference::English => g.title.clone(),
		};

		Manga {
			key: g.id.clone(),
			title,
			cover: Some(g.cover_url()),
			description: Some(description_parts.join("  \n")),
			authors: if !combined_authors.is_empty() {
				Some(combined_authors)
			} else {
				None
			},
			artists: if !artists.is_empty() {
				Some(artists)
			} else {
				None
			},
			url: Some(format!("https://hitomi.la{}", g.gallery_url)),
			tags: if !tags.is_empty() { Some(tags) } else { None },
			status: MangaStatus::Completed,
			content_rating: ContentRating::NSFW,
			viewer,
			update_strategy: UpdateStrategy::Never,
			..Default::default()
		}
	}
}
