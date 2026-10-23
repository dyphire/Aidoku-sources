#![no_std]

mod gbk;
mod home;
mod html;
mod markdown;
mod net;

use aidoku::{
	BasicLoginHandler, Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider,
	Listing, ListingProvider, Manga, MangaPageResult, Page, PageContent, PageContext, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::net::Request,
	imports::std::send_partial_result,
	prelude::*,
};

use html::{
	ListPage as _, NovelPage as _, ReaderPage as _, WapCatalogPage as _, WapSearchPage as _,
	fetch_gb, fetch_wap, wap_chapter_text,
};
use net::Url;

pub const BASE_URL: &str = "https://www.wenku8.net";
/// The mobile WAP site serves the anonymous homepage sections and uses
/// UTF-8 instead of GBK.
pub const WAP_URL: &str = "https://www.wenku8.net/wap";

/// Toplist sort values accepted by `toplist.php`.
const LISTING_SORTS: &[&str] = &[
	"dayvisit",   // 今日热榜
	"lastupdate", // 最近更新
	"allvisit",   // 总点击榜
	"allvote",    // 总推荐榜
	"monthvisit", // 月点击榜
	"postdate",   // 最新入库
];

struct Wenku8;

/// Chapter list fallback through the anonymous WAP catalog, one request
/// per volume-batch page. Pages concatenate in document order and the
/// whole list is returned newest-first, matching the desktop parser.
fn fetch_chapters_via_wap(aid: &str) -> Result<Vec<Chapter>> {
	let mut chapters = vec![];
	let mut volume_number = 0.0;
	let document = fetch_wap(&Url::wap_read_book(aid, 1))?;
	let total = document.wap_catalog_total_pages();
	chapters.extend(document.wap_catalog(aid, &mut volume_number));
	for page in 2..=total.max(1) {
		let document = fetch_wap(&Url::wap_read_book(aid, page))?;
		chapters.extend(document.wap_catalog(aid, &mut volume_number));
	}
	Ok(chapters.into_iter().rev().collect())
}

/// Chapter text fallback through the anonymous WAP reader, one request
/// per character-count page.
fn fetch_pages_via_wap(aid: &str, cid: &str) -> Result<Vec<Page>> {
	let document = fetch_wap(&Url::wap_read_chapter(aid, cid, 1))?;
	let (text, total) = wap_chapter_text(&document);
	let mut raw = text;
	for page in 2..=total.max(1) {
		let document = fetch_wap(&Url::wap_read_chapter(aid, cid, page))?;
		let (text, _) = wap_chapter_text(&document);
		raw.push('\n');
		raw.push_str(&text);
	}
	let body = markdown::text_to_markdown(&raw);
	if body.is_empty() {
		bail!("Chapter content is empty");
	}
	Ok(vec![Page {
		content: PageContent::text(body),
		..Default::default()
	}])
}

impl Source for Wenku8 {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		let mut search_type = "articlename";
		let mut fullflag = "0";
		for filter in &filters {
			match filter {
				FilterValue::Select { id, value } if id == "search-type" => {
					search_type = match value.as_str() {
						"author" => "author",
						_ => "articlename",
					};
				}
				FilterValue::Select { id, value } if id == "collection" => {
					fullflag = match value.as_str() {
						"1" => "1",
						_ => "0",
					};
				}
				_ => {}
			}
		}

		let url = if let Some(query) = query.filter(|q| !q.trim().is_empty()) {
			// The desktop search requires a logged-in session; the WAP
			// endpoint is anonymous and answers in UTF-8.
			let response = net::wap_search(search_type, query.trim())?;
			let document = html::parse_utf8_response(response)?;
			return Ok(MangaPageResult {
				entries: document.wap_search_results(),
				has_next_page: false,
			});
		} else {
			Url::Catalog { fullflag, page }
		};
		let document = fetch_gb(&url)?;
		Ok(document.manga_page_result())
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details {
			let document = fetch_gb(&Url::novel(&manga.key))?;
			document.update_details(&mut manga)?;
			if manga.cover.is_none() {
				manga.cover = Some(net::cover_url(&manga.key));
			}
			if needs_chapters {
				send_partial_result(&manga);
			}
		}

		if needs_chapters {
			// The desktop reader needs no login, but falls to a closure
			// notice under Cloudflare; the WAP catalog is anonymous.
			manga.chapters = Some(match fetch_gb(&Url::reader(&manga.key)) {
				Ok(document) => document.chapters()?,
				Err(_) => fetch_chapters_via_wap(&manga.key)?,
			});
		}

		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// Chapter keys are stored as "{aid}:{cid}".
		let (aid, cid) = chapter
			.key
			.split_once(':')
			.map(|(a, c)| (a.to_string(), c.to_string()))
			.unwrap_or_else(|| (manga.key.clone(), chapter.key.clone()));
		let document = match fetch_gb(&Url::chapter(&aid, &cid)) {
			Ok(document) => document,
			// Cloudflare-served chapter pages fall back to the anonymous
			// WAP text (paginated by character count).
			Err(_) => return fetch_pages_via_wap(&aid, &cid),
		};
		let content = document
			.select_first("#content")
			.ok_or_else(|| error!("Chapter page missing #content"))?;

		// Illustration-only chapters (e.g. 彩页/插图) become image pages so
		// the reader stays in image mode; prose chapters stay a single text
		// page (mixed page types would force image mode and hide the text).
		let mut image_urls = vec![];
		markdown::extract_image_urls(&content, &mut image_urls);
		let mut raw = String::new();
		markdown::extract_text(&content, &mut raw);
		let body = markdown::text_to_markdown(&raw);
		if body.is_empty() && !image_urls.is_empty() {
			return Ok(image_urls
				.into_iter()
				.map(|url| Page {
					content: PageContent::url(url),
					..Default::default()
				})
				.collect());
		}
		if body.is_empty() {
			// Report a genuine failure instead of fake content: placeholder
			// text would render fine and mark the chapter read.
			bail!("Chapter content is empty");
		}
		Ok(vec![Page {
			content: PageContent::text(body),
			..Default::default()
		}])
	}
}

impl ListingProvider for Wenku8 {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		if listing.id == "fullbooks" {
			let document = fetch_gb(&Url::Catalog {
				fullflag: "1",
				page,
			})?;
			return Ok(document.manga_page_result());
		}
		let Some(&sort) = LISTING_SORTS.iter().find(|&&s| s == listing.id) else {
			bail!("Unknown listing: {}", listing.id);
		};
		let document = fetch_gb(&Url::Toplist { sort, page })?;
		Ok(document.manga_page_result())
	}
}

impl BasicLoginHandler for Wenku8 {
	fn handle_basic_login(&self, _key: String, username: String, password: String) -> Result<bool> {
		net::login(&username, &password)
	}
}

impl ImageRequestProvider for Wenku8 {
	fn get_image_request(&self, url: String, _context: Option<PageContext>) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", &format!("{BASE_URL}/")))
	}
}

impl DeepLinkHandler for Wenku8 {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url
			.split(['?', '#'])
			.next()
			.unwrap_or(&url)
			.rsplit("wenku8.net")
			.next()
			.unwrap_or("")
			.trim_start_matches('/');
		let Some(segment) = path.split('/').nth(1) else {
			return Ok(None);
		};
		let aid: String = segment
			.trim_end_matches(".htm")
			.chars()
			.take_while(|c| c.is_ascii_digit())
			.collect();
		if aid.is_empty() {
			return Ok(None);
		}
		Ok(Some(DeepLinkResult::Manga { key: aid }))
	}
}

register_source!(
	Wenku8,
	ListingProvider,
	DeepLinkHandler,
	Home,
	ImageRequestProvider,
	BasicLoginHandler
);
