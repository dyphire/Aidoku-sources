#![no_std]

mod html;
mod net;

use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, FilterValue, ImageRequestProvider, Listing,
	ListingProvider, Manga, MangaPageResult, Page, PageContent, Result, Source,
	alloc::{String, Vec, string::ToString as _, vec},
	imports::{html::Document, net::Request, std::sleep},
	prelude::*,
};
use core::cell::RefCell;

use html::{ChapterPage as _, MangaDetails as _, MangaPage as _};
use net::Url;

pub const BASE_URL: &str = "https://quanben5.com";
pub const REFERER_URL: &str = "https://quanben5.com/";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
	 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

/// Number of request attempts per page (initial + retries). The site
/// throttles bursts (so-novel marks it `needProxy`), so requests back off
/// with an increasing delay (2s, then 4s).
const MAX_ATTEMPTS: u32 = 3;

/// Retry a fallible request with increasing backoff (2s, then 4s), mirroring
/// so-novel's crawl strategy for this rate-limited site.
fn retry_loop<T>(mut action: impl FnMut() -> Result<T>) -> Result<T> {
	let mut attempt: u32 = 0;
	loop {
		match action() {
			Ok(value) => return Ok(value),
			Err(e) => {
				attempt += 1;
				if attempt >= MAX_ATTEMPTS {
					return Err(e);
				}
				sleep(2 * attempt as i32);
			}
		}
	}
}

/// Fetch a document with retries. A browser-like User-Agent is set because
/// the site behaves differently (or fails) under non-browser UAs.
fn fetch_document(url: &Url) -> Result<Document> {
	retry_loop(|| {
		url.request()?
			.header("User-Agent", USER_AGENT)
			.html()
			.map_err(Into::into)
	})
}

/// Fetch the raw response text (used for the JSONP search endpoint).
/// The search endpoint rejects requests without a Referer and returns an
/// empty "no results" page instead of the JSONP payload.
fn fetch_search(url: &Url) -> Result<String> {
	retry_loop(|| {
		url.request()?
			.header("Referer", REFERER_URL)
			.header("User-Agent", USER_AGENT)
			.string()
	})
}

struct Quanben5 {
	/// Small LCG used to generate the two random padding chars the search
	/// endpoint expects around every obfuscated keyword char.
	rng: RefCell<u32>,
}

impl Source for Quanben5 {
	fn new() -> Self {
		Self {
			rng: RefCell::new(0x9E37_79B9),
		}
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<MangaPageResult> {
		match query {
			Some(q) if !q.is_empty() => {
				let mut rng = self.rng.borrow_mut();
				*rng = rng.wrapping_mul(1664525).wrapping_add(1013904223);
				let url = Url::Search {
					query: q.trim().to_string(),
					rng: *rng,
				};
				let body = fetch_search(&url)?;
				match html::search_page_result(&body) {
					// The site rate-limits bursts and can answer with a fake
					// "没有搜索到" page; retry once before trusting an empty
					// result (a genuine no-match stays empty after the retry).
					Ok(result) if result.entries.is_empty() => {
						sleep(2);
						let body = fetch_search(&url)?;
						html::search_page_result(&body)
					}
					other => other,
				}
			}
			_ => {
				let url = Url::from_filters(filters, page);
				let document = fetch_document(&url)?;
				document.list_page_result(page)
			}
		}
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details {
			let document = fetch_document(&Url::manga(&manga.key))?;
			document.update_details(&mut manga)?;
		}
		if needs_chapters {
			let document = fetch_document(&Url::toc(&manga.key))?;
			let chapters = document.chapters()?;
			if chapters.is_empty() {
				bail!("book {}: no chapters found", manga.key);
			}
			manga.chapters = Some(chapters);
		}
		Ok(manga)
	}

	fn get_page_list(&self, manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		// One body page per chapter on this site; empty content is a
		// rate-limit symptom, retried once after a backoff (fetch_document
		// already retries failed requests with increasing delays).
		let url = Url::chapter(&manga.key, &chapter.key);
		let document = fetch_document(&url)?;
		let text = html::chapter_text(&document);
		if text.is_empty() {
			sleep(2);
			let document = fetch_document(&url)?;
			let text = html::chapter_text(&document);
			if text.is_empty() {
				bail!("chapter {} of {}: empty content", chapter.key, manga.key);
			}
			return Ok(vec![Page {
				content: PageContent::text(text),
				..Default::default()
			}]);
		}
		Ok(vec![Page {
			content: PageContent::text(text),
			..Default::default()
		}])
	}
}

impl ListingProvider for Quanben5 {
	fn get_manga_list(&self, _listing: Listing, _page: i32) -> Result<MangaPageResult> {
		// No extra listings are declared; browsing goes through
		// `get_search_manga_list` with the category filter.
		Ok(MangaPageResult::default())
	}
}

impl ImageRequestProvider for Quanben5 {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<Request> {
		Ok(Request::get(url)?.header("Referer", REFERER_URL))
	}
}

impl DeepLinkHandler for Quanben5 {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let path = url
			.split(['?', '#'])
			.next()
			.unwrap_or(&url)
			.rsplit("quanben5.com")
			.next()
			.unwrap_or("")
			.trim_matches('/');

		let mut parts = path.splitn(3, '/');
		match (parts.next(), parts.next(), parts.next()) {
			// `/n/{id}/`
			(Some("n"), Some(id), None) if is_book_id(id) => {
				Ok(Some(DeepLinkResult::Manga { key: id.into() }))
			}
			// `/n/{id}/{chapter}.html`
			(Some("n"), Some(id), Some(chapter)) => {
				let chapter = chapter.trim_end_matches(".html");
				if is_book_id(id) && is_numeric(chapter) {
					Ok(Some(DeepLinkResult::Chapter {
						manga_key: id.into(),
						key: chapter.into(),
					}))
				} else {
					Ok(None)
				}
			}
			_ => Ok(None),
		}
	}
}

fn is_book_id(s: &str) -> bool {
	!s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric())
}

fn is_numeric(s: &str) -> bool {
	!s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

register_source!(
	Quanben5,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);
