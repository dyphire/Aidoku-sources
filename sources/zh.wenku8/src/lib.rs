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

use html::{ListPage as _, NovelPage as _, ReaderPage as _, fetch_gb};
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
			Url::search(search_type, query.trim(), page)
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
			let document = fetch_gb(&Url::reader(&manga.key))?;
			manga.chapters = Some(document.chapters()?);
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
		let document = fetch_gb(&Url::chapter(&aid, &cid))?;
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
		let text = if body.is_empty() {
			"(empty chapter)".to_string()
		} else {
			body
		};
		Ok(vec![Page {
			content: PageContent::text(text),
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
