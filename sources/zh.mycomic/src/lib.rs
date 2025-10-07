#![no_std]

mod html;
mod net;

use aidoku::{
	Chapter, DeepLinkHandler, DeepLinkResult, ImageRequestProvider, Manga, MangaPageResult, Page,
	Result, Source,
	alloc::{String, Vec},
	prelude::*,
};
use html::{ChapterPage as _, MangaPage as _, PageList as _};
use net::Url;

pub const BASE_URL: &str = "https://mycomic.com";
pub const USER_AGENT: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1";

struct Mycomic;

impl Source for Mycomic {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<aidoku::FilterValue>,
	) -> Result<MangaPageResult> {
		let url = Url::from_query_or_filters(query.as_deref(), page, &filters)?;
		let html = url.request()?.html()?;
		html.manga_page_result()
	}

	fn get_manga_update(
		&self,
		mut manga: Manga,
		needs_details: bool,
		needs_chapters: bool,
	) -> Result<Manga> {
		if needs_details {
			let manga_page = Url::manga(manga.key.clone()).request()?.html()?;
			manga_page.update_details(&mut manga)?;
		}

		if needs_chapters {
			let chapter_list_page = Url::chapter_list(manga.key.clone()).request()?.html()?;
			manga.chapters = Some(chapter_list_page.chapters(&manga.key)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		Url::chapter(chapter.key)
			.request()?
			.html()?
			.pages()
	}
}

impl ImageRequestProvider for Mycomic {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<aidoku::imports::net::Request> {
		Ok(aidoku::imports::net::Request::get(url)?
			.header("User-Agent", USER_AGENT)
			.header("Referer", BASE_URL))
	}
}

impl DeepLinkHandler for Mycomic {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let url = url.trim_start_matches(BASE_URL);
		let mut splits = url.split('/').skip(1);
		let deep_link_result = match splits.next() {
			Some("comics") => match splits.next() {
				Some(id) => Some(DeepLinkResult::Manga { key: id.into() }),
				_ => None,
			},
			Some("chapters") => match (splits.next(), splits.next()) {
				(Some(id), None) => Some(DeepLinkResult::Chapter {
					manga_key: "".into(),
					key: id.into(),
				}),
				_ => None,
			},
			_ => None,
		};
		Ok(deep_link_result)
	}
}

register_source!(Mycomic, ImageRequestProvider, DeepLinkHandler);
