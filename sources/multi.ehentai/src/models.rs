use crate::settings::{TitlePreference, get_title_preference};
use aidoku::{
	ContentRating, Manga, MangaStatus, UpdateStrategy, Viewer,
	alloc::{Vec, string::String},
	prelude::*,
};

#[derive(Debug, Clone)]
pub struct EHTag {
	pub namespace: String,
	pub name: String,
	pub is_weak: bool,
}

#[derive(Debug, Default, Clone)]
pub struct EHGallery {
	pub gid: String,
	pub token: String,
	pub title: String,
	pub alt_title: String,
	pub cover: String,
	pub category: String,
	pub uploader: String,
	pub posted: String,
	pub language: String,
	pub translated: bool,
	pub file_size: String,
	pub length: i32,
	pub favorites: i32,
	pub avg_rating: f64,
	pub rating_count: i32,
	pub tags: Vec<EHTag>,
}

/// Compact gallery info parsed from gallery list pages
#[derive(Debug, Clone)]
pub struct EHGalleryItem {
	pub url: String,
	pub title: String,
	pub alt_title: String,
	pub cover: String,
	pub category: String,
	pub tags: Vec<String>,
	pub language: Option<String>,
}

/// Select display title based on preference, falling back to the other title if needed.
fn select_title(title: String, alt_title: String) -> String {
	let pref = get_title_preference();
	match pref {
		TitlePreference::Japanese if !alt_title.is_empty() => alt_title,
		_ => {
			if title.is_empty() {
				alt_title
			} else {
				title
			}
		}
	}
}

/// Return `Some(v)` if `v` is non-empty, else `None`.
fn non_empty<T: AsRef<[U]>, U>(v: T) -> Option<T> {
	if v.as_ref().is_empty() { None } else { Some(v) }
}

impl From<EHGalleryItem> for Manga {
	fn from(item: EHGalleryItem) -> Self {
		let title = select_title(item.title, item.alt_title);

		let mut authors: Vec<String> = Vec::new();
		let mut groups: Vec<String> = Vec::new();
		let mut parodies: Vec<String> = Vec::new();
		let mut characters: Vec<String> = Vec::new();

		for t in &item.tags {
			if let Some(name) = t.strip_prefix("artist:") {
				authors.push(String::from(name));
			} else if let Some(name) = t.strip_prefix("group:") {
				groups.push(String::from(name));
			} else if let Some(name) = t.strip_prefix("parody:") {
				if name != "original" && name != "various" {
					parodies.push(String::from(name));
				}
			} else if let Some(name) = t.strip_prefix("character:") {
				characters.push(String::from(name));
			}
		}

		// has artist → use artist as authors; no artist → use group as authors
		let use_artist = !authors.is_empty();
		let combined_authors: Vec<String> = if use_artist {
			authors.clone()
		} else {
			groups.clone()
		};

		// tags: exclude language, misc, parody:original/various, meaningless other: tags, and whichever namespace was chosen as authors
		let author_prefix = if use_artist { "artist:" } else { "group:" };
		const EXCLUDED_OTHER: &[&str] = &[
			"other:already uploaded",
			"other:missing cover",
			"other:forbidden content",
			"other:replaced",
			"other:compilation",
			"other:incomplete",
			"other:caption",
		];
		let tags: Vec<String> = item
			.tags
			.iter()
			.filter(|t| {
				!t.starts_with("language:")
					&& !t.starts_with("misc:")
					&& *t != "parody:original"
					&& *t != "parody:various"
					&& !EXCLUDED_OTHER.contains(&t.as_str())
					&& !t.starts_with(author_prefix)
			})
			.cloned()
			.collect();

		let mut desc_parts: Vec<String> = Vec::new();
		if let Some(ref lang) = item.language {
			desc_parts.push(format!("Language: {lang}"));
		}
		// the namespace NOT chosen as authors goes into description
		if use_artist && !groups.is_empty() {
			desc_parts.push(format!("Group: {}", groups.join(", ")));
		} else if !use_artist && !authors.is_empty() {
			desc_parts.push(format!("Artist: {}", authors.join(", ")));
		}
		if !parodies.is_empty() {
			desc_parts.push(format!("Parody: {}", parodies.join(", ")));
		}
		if !characters.is_empty() {
			desc_parts.push(format!("Characters: {}", characters.join(", ")));
		}

		let description = if desc_parts.is_empty() {
			None
		} else {
			Some(desc_parts.join("  \n"))
		};

		Manga {
			key: item.url.clone(),
			title,
			cover: Some(item.cover),
			url: Some(item.url),
			description,
			tags: non_empty(tags),
			authors: non_empty(combined_authors),
			content_rating: ContentRating::NSFW,
			status: MangaStatus::Completed,
			update_strategy: UpdateStrategy::Never,
			..Default::default()
		}
	}
}

impl From<EHGallery> for Manga {
	fn from(gallery: EHGallery) -> Self {
		let title = select_title(gallery.title.clone(), gallery.alt_title.clone());

		let artists: Vec<String> = gallery
			.tags
			.iter()
			.filter(|t| t.namespace == "artist")
			.map(|t| t.name.clone())
			.collect();

		let groups: Vec<String> = gallery
			.tags
			.iter()
			.filter(|t| t.namespace == "group")
			.map(|t| t.name.clone())
			.collect();

		// has artist → use artist as authors; no artist → use group as authors
		let use_artist = !artists.is_empty();
		let combined_authors: Vec<String> = if use_artist {
			artists.clone()
		} else {
			groups.clone()
		};

		// tags: exclude language, misc, parody:original/various, meaningless other: tags, and whichever namespace was chosen as authors
		const EXCLUDED_OTHER_NAMES: &[&str] = &[
			"already uploaded",
			"missing cover",
			"forbidden content",
			"replaced",
			"compilation",
			"incomplete",
			"caption",
		];
		let author_ns = if use_artist { "artist" } else { "group" };
		let tags: Vec<String> = gallery
			.tags
			.iter()
			.filter(|t| {
				!t.namespace.is_empty()
					&& t.namespace != "language"
					&& t.namespace != "misc"
					&& t.namespace != author_ns
					&& !(t.namespace == "parody" && (t.name == "original" || t.name == "various"))
					&& !(t.namespace == "other" && EXCLUDED_OTHER_NAMES.contains(&t.name.as_str()))
			})
			.map(|t| format!("{}:{}", t.namespace, t.name))
			.collect();

		let mut desc_parts: Vec<String> = Vec::new();
		// the namespace NOT chosen as authors goes into description
		if use_artist && !groups.is_empty() {
			desc_parts.push(format!("Group: {}", groups.join(", ")));
		}
		if gallery.length > 0 {
			desc_parts.push(format!("Pages: {}", gallery.length));
		}
		if gallery.avg_rating > 0.0 {
			desc_parts.push(format!(
				"Rating: {:.1} ({} votes)",
				gallery.avg_rating, gallery.rating_count
			));
		}
		if gallery.favorites > 0 {
			desc_parts.push(format!("Favorites: {}", gallery.favorites));
		}
		let parodies: Vec<String> = gallery
			.tags
			.iter()
			.filter(|t| t.namespace == "parody" && t.name != "original" && t.name != "various")
			.map(|t| t.name.clone())
			.collect();
		if !parodies.is_empty() {
			desc_parts.push(format!("Parody: {}", parodies.join(", ")));
		}
		let characters: Vec<String> = gallery
			.tags
			.iter()
			.filter(|t| t.namespace == "character")
			.map(|t| t.name.clone())
			.collect();
		if !characters.is_empty() {
			desc_parts.push(format!("Characters: {}", characters.join(", ")));
		}
		if !gallery.file_size.is_empty() {
			desc_parts.push(format!("File Size: {}", gallery.file_size));
		}
		if !gallery.uploader.is_empty() {
			desc_parts.push(format!("Uploader: {}", gallery.uploader));
		}

		let description = if desc_parts.is_empty() {
			None
		} else {
			Some(desc_parts.join("  \n"))
		};

		let viewer = if gallery
			.tags
			.iter()
			.any(|t| t.namespace == "language" && t.name == "japanese")
		{
			Viewer::RightToLeft
		} else {
			Viewer::LeftToRight
		};

		let url = format!("https://e-hentai.org/g/{}/{}/", gallery.gid, gallery.token);

		Manga {
			key: url.clone(),
			title,
			cover: Some(gallery.cover),
			description,
			authors: non_empty(combined_authors),
			artists: non_empty(artists),
			url: Some(url),
			tags: non_empty(tags),
			status: MangaStatus::Completed,
			content_rating: ContentRating::NSFW,
			viewer,
			update_strategy: UpdateStrategy::Never,
			..Default::default()
		}
	}
}
