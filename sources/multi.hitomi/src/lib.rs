#![no_std]
use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, HashMap, ImageRequestProvider, Listing,
	ListingProvider, Manga, MangaPageResult, Page, PageContent, PageContext, Result, Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::{net::Request, std::parse_date},
	prelude::*,
};

mod gg;
mod models;
mod search;
mod settings;

use gg::{fetch_gallery, fetch_gg_state, image_url};
use search::{decode_nozomi, fetch_nozomi_page, nozomi_url_for_ns_tag, search_plain_text};

pub const BASE_URL: &str = "https://hitomi.la";
pub const REFERER: &str = "https://hitomi.la/";
pub const LTN_URL: &str = "https://ltn.gold-usergeneratedcontent.net";
pub const CDN_DOMAIN: &str = "gold-usergeneratedcontent.net";
pub const PAGE_SIZE: i32 = 25;

struct Hitomi;

impl Source for Hitomi {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let lang = settings::get_nozomi_language();

		let mut sort_area: Option<String> = None;
		let mut sort_tag: String = "index".into();
		let mut type_filter: Option<String> = None;

		let mut artist_filter: Option<String> = None;
		let mut group_filter: Option<String> = None;
		let mut author_name: Option<String> = None;
		let mut genre_filter: Option<String> = None;

		for f in &filters {
			match f {
				FilterValue::Sort { index, .. } => match *index {
					1 => {
						sort_area = Some("popular".into());
						sort_tag = "published".into();
					}
					2 => {
						sort_area = Some("popular".into());
						sort_tag = "today".into();
					}
					3 => {
						sort_area = Some("popular".into());
						sort_tag = "week".into();
					}
					4 => {
						sort_area = Some("popular".into());
						sort_tag = "month".into();
					}
					5 => {
						sort_area = Some("popular".into());
						sort_tag = "year".into();
					}
					_ => {
						sort_area = None;
						sort_tag = "index".into();
					}
				},
				FilterValue::Text { id, value } if !value.is_empty() => match id.as_str() {
					"author" => {
						author_name = Some(value.to_lowercase().replace(' ', "_"));
					}
					"artist" => {
						artist_filter =
							Some(format!("artist:{}", value.to_lowercase().replace(' ', "_")));
					}
					"group" => {
						group_filter =
							Some(format!("group:{}", value.to_lowercase().replace(' ', "_")));
					}
					_ => {}
				},
				FilterValue::Select { id, value } => match id.as_str() {
					"type" if value != "Any" && !value.is_empty() => {
						type_filter = Some(value.clone());
					}
					"genre" => {
						let v = value.to_lowercase().replace(' ', "_");
						genre_filter = Some(if v.contains(':') {
							v
						} else {
							format!("tag:{v}")
						});
					}
					_ => {}
				},
				_ => {}
			}
		}

		let raw_q: String = query.as_deref().unwrap_or("").trim().into();

		// numeric ID or URL shortcut — resolve to gallery ID and return directly
		let shortcut_id: Option<i64> = if raw_q.is_empty() {
			None
		} else if let Ok(id) = raw_q.parse::<i64>() {
			// bare numeric ID
			Some(id)
		} else if raw_q.contains("hitomi.la") {
			// full URL — extract ID using the same logic as DeepLinkHandler
			extract_hitomi_id(&raw_q)
		} else {
			None
		};
		if let Some(id) = shortcut_id
			&& let Some(g) = fetch_gallery(id)
		{
			return Ok(MangaPageResult {
				entries: vec![g.into()],
				has_next_page: false,
			});
		}

		let mut positive_terms: Vec<String> = Vec::new();
		let mut negative_terms: Vec<String> = Vec::new();

		// Tokenize: words are separated by whitespace.
		// A word containing ':' starts a ns:tag term; subsequent plain words
		// (no ':' and no leading '-') are appended with '_' to form a multi-word tag.
		// Plain words without any ns:tag context are joined together as a single
		// plain-text query so "big breasts" is searched as one term.
		{
			let tokens: Vec<&str> = raw_q.split_whitespace().collect();
			let mut i = 0;
			while i < tokens.len() {
				let tok = tokens[i].to_lowercase();
				if let Some(neg) = tok.strip_prefix('-') {
					if !neg.is_empty() {
						negative_terms.push(neg.into());
					}
					i += 1;
				} else if tok.contains(':') {
					// ns:tag — collect following plain words as part of the tag
					let mut term = tok;
					i += 1;
					while i < tokens.len() {
						let next = tokens[i];
						if next.starts_with('-') || next.contains(':') {
							break;
						}
						term.push('_');
						term.push_str(&next.to_lowercase());
						i += 1;
					}
					positive_terms.push(term);
				} else {
					// plain-text: collect all consecutive plain words into one term
					let mut term = tok;
					i += 1;
					while i < tokens.len() {
						let next = tokens[i];
						if next.starts_with('-') || next.contains(':') {
							break;
						}
						term.push(' ');
						term.push_str(&next.to_lowercase());
						i += 1;
					}
					positive_terms.push(term);
				}
			}
		}

		if let Some(t) = artist_filter {
			positive_terms.push(t);
		}
		if let Some(t) = group_filter {
			positive_terms.push(t);
		}
		if let Some(t) = genre_filter {
			positive_terms.push(t);
		}

		let is_default_sort = sort_area.is_none() && sort_tag == "index";
		if lang != "all"
			&& is_default_sort
			&& !positive_terms
				.iter()
				.chain(negative_terms.iter())
				.any(|t| t.starts_with("language:"))
		{
			positive_terms.push(format!("language:{lang}"));
		}

		let get_ids_for_term = |term: &str| -> Option<Vec<i64>> {
			if term.contains(':') {
				let url = nozomi_url_for_ns_tag(term, &lang)?;
				let data = Request::get(&url)
					.ok()?
					.header("Referer", REFERER)
					.data()
					.ok()?;
				Some(decode_nozomi(&data))
			} else {
				search_plain_text(term)
			}
		};

		let sort_nozomi_url = match &sort_area {
			Some(area) => format!("{LTN_URL}/{area}/{sort_tag}-{lang}.nozomi"),
			None => format!("{LTN_URL}/{sort_tag}-{lang}.nozomi"),
		};

		let use_sort_base =
			(positive_terms.is_empty() && author_name.is_none()) || !is_default_sort;

		let mut positive_results: Vec<Vec<i64>> = Vec::new();
		for term in &positive_terms {
			match get_ids_for_term(term) {
				Some(ids) => positive_results.push(ids),
				None => return Err(error!("Search failed for term: {term}")),
			}
		}

		// "author" from detail page: union of artist: and group: nozomis
		if let Some(ref name) = author_name {
			let artist_ids = get_ids_for_term(&format!("artist:{name}"));
			let group_ids = get_ids_for_term(&format!("group:{name}"));
			let mut union: Vec<i64> = match (artist_ids, group_ids) {
				(Some(a), Some(b)) => {
					let mut v = a;
					v.extend(b);
					v
				}
				(Some(a), None) => a,
				(None, Some(b)) => b,
				(None, None) => return Err(error!("No results for author: {name}")),
			};
			union.sort_unstable();
			union.dedup();
			positive_results.push(union);
		}

		let mut negative_ids: Vec<i64> = Vec::new();
		for term in &negative_terms {
			if let Some(ids) = get_ids_for_term(term) {
				negative_ids.extend(ids);
			}
		}
		negative_ids.sort_unstable();
		negative_ids.dedup();

		let mut result_ids: Vec<i64> = if use_sort_base {
			let data = Request::get(&sort_nozomi_url)?
				.header("Referer", REFERER)
				.data()?;
			decode_nozomi(&data)
		} else {
			Vec::new()
		};

		for pos_ids in positive_results {
			if result_ids.is_empty() {
				result_ids = pos_ids;
			} else {
				let pos_set: Vec<i64> = {
					let mut v = pos_ids;
					v.sort_unstable();
					v
				};
				result_ids.retain(|id| pos_set.binary_search(id).is_ok());
			}
		}

		if let Some(ref t) = type_filter {
			let type_url = format!("{LTN_URL}/type/{t}-{lang}.nozomi");
			if let Ok(data) =
				Request::get(&type_url).and_then(|r| r.header("Referer", REFERER).data())
			{
				let mut type_ids = decode_nozomi(&data);
				type_ids.sort_unstable();
				result_ids.retain(|id| type_ids.binary_search(id).is_ok());
			}
		}

		if !negative_ids.is_empty() {
			result_ids.retain(|id| negative_ids.binary_search(id).is_err());
		}

		let start = ((page - 1) * PAGE_SIZE) as usize;
		let end = (start + PAGE_SIZE as usize).min(result_ids.len());
		let has_next_page = end < result_ids.len();
		let page_ids = if start < result_ids.len() {
			&result_ids[start..end]
		} else {
			&[]
		};

		let entries: Vec<Manga> = page_ids
			.iter()
			.filter_map(|&id| fetch_gallery(id))
			.map(|g| g.into())
			.collect();

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		let id: i64 = manga
			.key
			.parse()
			.map_err(|_| error!("Invalid gallery id"))?;
		if needs_details || needs_chapters {
			let gallery = fetch_gallery(id).ok_or_else(|| error!("Failed to fetch gallery"))?;
			if needs_details {
				let new_manga: Manga = gallery.clone().into();
				manga.copy_from(new_manga);
			}
			if needs_chapters {
				let scanlators = gallery
					.language
					.as_ref()
					.filter(|l| !l.is_empty())
					.map(|l| vec![l.clone()]);
				let date_uploaded =
					parse_date(&gallery.date[..10.min(gallery.date.len())], "yyyy-MM-dd");
				let chapter = Chapter {
					key: manga.key.clone(),
					chapter_number: Some(1.0),
					date_uploaded,
					url: Some(format!("{BASE_URL}/reader/{id}.html")),
					scanlators,
					..Default::default()
				};
				manga.chapters = Some(vec![chapter]);
			}
		}
		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let id: i64 = chapter
			.key
			.parse()
			.map_err(|_| error!("Invalid gallery id"))?;
		let gallery = fetch_gallery(id).ok_or_else(|| error!("Failed to fetch gallery"))?;

		// Fetch gg.js to get current subdomain routing state
		let gg = fetch_gg_state().ok_or_else(|| error!("Failed to fetch gg.js"))?;

		let reader_url = format!("{BASE_URL}/reader/{id}.html");
		let pages = gallery
			.files
			.iter()
			.map(|f| {
				let ext = if f.is_gif() { "webp" } else { "avif" };
				let url = image_url(&f.hash, ext, &gg);
				let mut ctx: PageContext = HashMap::new();
				ctx.insert("referer".into(), reader_url.clone());
				Page {
					content: PageContent::url_context(url, ctx),
					..Default::default()
				}
			})
			.collect();
		Ok(pages)
	}
}

impl ListingProvider for Hitomi {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let lang = settings::get_nozomi_language();
		let url = match listing.id.as_str() {
			"popular_today" => format!("{LTN_URL}/popular/today-{lang}.nozomi"),
			"popular_week" => format!("{LTN_URL}/popular/week-{lang}.nozomi"),
			"popular_month" => format!("{LTN_URL}/popular/month-{lang}.nozomi"),
			"popular_year" => format!("{LTN_URL}/popular/year-{lang}.nozomi"),
			_ => format!("{LTN_URL}/index-{lang}.nozomi"),
		};
		let (ids, has_next_page) = fetch_nozomi_page(&url, page)?;

		let entries: Vec<Manga> = ids
			.into_iter()
			.filter_map(fetch_gallery)
			.map(|g| g.into())
			.collect();

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}
}

/// Extract a gallery ID from any hitomi.la URL format:
/// - https://hitomi.la/reader/123456.html
/// - https://hitomi.la/g/123456/
/// - https://hitomi.la/galleries/manga-title-123456.html
fn extract_hitomi_id(url: &str) -> Option<i64> {
	if let Some(idx) = url.find("/reader/") {
		let rest = &url[idx + "/reader/".len()..];
		let end = rest.find('.').unwrap_or(rest.len());
		return rest[..end].parse().ok();
	}
	if let Some(idx) = url.find("/g/") {
		let rest = &url[idx + "/g/".len()..];
		let end = rest.find('/').unwrap_or(rest.len());
		return rest[..end].parse().ok();
	}
	if let Some(dot) = url.rfind(".html") {
		let path = &url[..dot];
		if let Some(dash) = path.rfind('-') {
			let maybe_id = &path[dash + 1..];
			if maybe_id.chars().all(|c| c.is_ascii_digit()) {
				return maybe_id.parse().ok();
			}
		}
	}
	None
}

impl DeepLinkHandler for Hitomi {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		if !url.contains("hitomi.la") {
			return Ok(None);
		}
		Ok(extract_hitomi_id(&url).map(|id| DeepLinkResult::Manga {
			key: id.to_string(),
		}))
	}
}

impl ImageRequestProvider for Hitomi {
	fn get_image_request(&self, url: String, context: Option<PageContext>) -> Result<Request> {
		let referer = context
			.as_ref()
			.and_then(|c| c.get("referer").map(|s| s.as_str()))
			.unwrap_or(REFERER);
		Ok(Request::get(url)?
			.header("Referer", referer)
			.header("Origin", BASE_URL)
			.header(
				"Accept",
				"image/webp,image/avif,image/apng,image/svg+xml,image/*,*/*;q=0.8",
			))
	}
}

register_source!(
	Hitomi,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
