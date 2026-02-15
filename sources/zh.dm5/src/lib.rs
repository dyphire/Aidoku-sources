#![no_std]

mod html;
mod net;
mod unpacker;

use aidoku::{
	alloc::{String, Vec, string::ToString},
	imports::net::Request,
	prelude::*,
	Chapter, DeepLinkHandler, DeepLinkResult, ImageRequestProvider, Listing, ListingProvider,
	Manga, MangaPageResult, Page, PageContent, Result, Source,
};
use html::MangaPage as _;
use net::Url;

pub const BASE_URL: &str = "https://www.dm5.com";
pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36";

struct Dm5;

impl Source for Dm5 {
	fn new() -> Self {
		Self
	}

	fn get_search_manga_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<aidoku::FilterValue>,
	) -> Result<MangaPageResult> {
		let url = if let Some(query) = query {
			format!("{}/search?title={}&language=1&page={}", BASE_URL, query, page)
		} else {
			// Default to popular
			format!("{}/manhua-list-p{}/", BASE_URL, page)
		};
		let html = aidoku::imports::net::Request::get(url)?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-TW")
			.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
			.header("Referer", BASE_URL)
			.html()?;
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
			manga.chapters = Some(html::ChapterList::get_chapters(&manga.key)?);
		}

		Ok(manga)
	}

	fn get_page_list(&self, _manga: Manga, chapter: Chapter) -> Result<Vec<Page>> {
		let chapter_url = format!("{}{}", BASE_URL, chapter.key);
		let html = Request::get(&chapter_url)?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-TW")
			.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
			.header("Referer", BASE_URL)
			.header("DNT", "1")
			.html()?;
		let images = html.select("div#barChapter > img.load-src");
		let mut pages: Vec<Page> = Vec::new();

		// Check for direct images first
		if let Some(img_list) = images {
			if !img_list.is_empty() {
				for (_index, img) in img_list.into_iter().enumerate() {
					let data_src = img.attr("data-src").unwrap_or_default();
					pages.push(Page {
						content: PageContent::Url(data_src, None),
						..Default::default()
					});
				}
				return Ok(pages);
			}
		}
		
		// Handle packed images - need to fetch and decode each page
		let script = html
			.select("script:contains(DM5_MID)")
			.and_then(|e| e.first())
			.and_then(|e| e.text())
			.ok_or_else(|| aidoku::error!("Script not found"))?;
		
		if !script.contains("DM5_VIEWSIGN_DT") {
			// Check if this is a paid/locked chapter
			if let Some(pay_msg) = html.select_first("div.view-pay-form p.subtitle") {
				if let Some(msg) = pay_msg.text() {
					return Err(aidoku::error!("{}", msg));
				}
			}
			return Err(aidoku::error!("Chapter not available"));
		}
		
		let cid = script
			.split("var DM5_CID=")
			.nth(1)
			.and_then(|s| s.split(';').next())
			.map(|s| s.trim().trim_matches('"'))
			.ok_or_else(|| aidoku::error!("CID not found"))?;
		let mid = script
			.split("var DM5_MID=")
			.nth(1)
			.and_then(|s| s.split(';').next())
			.map(|s| s.trim().trim_matches('"'))
			.ok_or_else(|| aidoku::error!("MID not found"))?;
		let dt = script
			.split("var DM5_VIEWSIGN_DT=")
			.nth(1)
			.and_then(|s| s.split(';').next())
			.map(|s| s.trim().trim_matches('"'))
			.ok_or_else(|| aidoku::error!("DT not found"))?;
		let sign = script
			.split("var DM5_VIEWSIGN=")
			.nth(1)
			.and_then(|s| s.split(';').next())
			.map(|s| s.trim().trim_matches('"'))
			.ok_or_else(|| aidoku::error!("SIGN not found"))?;
		let image_count: usize = script
			.split("var DM5_IMAGE_COUNT=")
			.nth(1)
			.and_then(|s| s.split(';').next())
			.and_then(|s| s.trim().parse().ok())
			.ok_or_else(|| aidoku::error!("Image count not found"))?;

		// Get the base URL for chapterfun.ashx from chapter URL
		let base_for_chapterfun = chapter_url.split('?').next().unwrap_or(&chapter_url);

		// Fetch and decode each page URL
		for i in 1..=image_count {
			let chapterfun_url = format!(
				"{}/chapterfun.ashx?cid={}&page={}&key=&language=1&gtk=6&_cid={}&_mid={}&_dt={}&_sign={}",
				base_for_chapterfun, cid, i, cid, mid, dt, sign
			);
			
			// Fetch the JavaScript code
			let response = Request::get(&chapterfun_url)?
				.header("User-Agent", USER_AGENT)
				.header("Referer", &base_for_chapterfun)
				.string()?;
			
			// Unpack and parse the image URL
			let unpacked = match unpacker::unpack(&response) {
				Ok(u) => u,
				Err(_) => response.clone(), // If unpack fails, try using original response
			};
			
			// Extract pix: var pix="https://..."
			let pix = unpacked
				.split("var pix=\"")
				.nth(1)
				.and_then(|s| s.split('"').next())
				.ok_or_else(|| aidoku::error!("Failed to extract pix from unpacked script"))?;
			
			// Extract pvalue array: var pvalue=["/file1.jpg","/file2.jpg"]
			// Get first element (current page)
			let pvalue = unpacked
				.split("var pvalue=[\"")
				.nth(1)
				.and_then(|s| s.split('"').next())
				.ok_or_else(|| aidoku::error!("Failed to extract pvalue from unpacked script"))?;
			
			// Extract query: pvalue[i]+'?cid=...'
			let query = unpacked
				.split("pvalue[i]+'")
				.nth(1)
				.and_then(|s| s.split('\'').next())
				.ok_or_else(|| aidoku::error!("Failed to extract query from unpacked script"))?;
			
			let image_url = format!("{}{}{}", pix, pvalue, query);
			
			pages.push(Page {
				content: PageContent::Url(image_url, None),
				..Default::default()
			});
		}

		Ok(pages)
	}
}

impl ImageRequestProvider for Dm5 {
	fn get_image_request(
		&self,
		url: String,
		_context: Option<aidoku::PageContext>,
	) -> Result<aidoku::imports::net::Request> {
		// Extract cid from image URL for proper referer
		// Image URLs typically have format: https://...?cid=123&...
		let cid = url
			.split("cid=")
			.nth(1)
			.and_then(|s| s.split('&').next())
			.unwrap_or("");
		
		let referer = if !cid.is_empty() {
			format!("{}/m{}", BASE_URL, cid)
		} else {
			BASE_URL.to_string()
		};
		
		Ok(aidoku::imports::net::Request::get(url)?
			.header("User-Agent", USER_AGENT)
			.header("Referer", &referer))
	}
}

impl DeepLinkHandler for Dm5 {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let url = url.trim_start_matches(BASE_URL);
		let mut splits = url.split('/').skip(1);
		let deep_link_result = match splits.next() {
			Some("m") => match (splits.next(), splits.next()) {
				(Some(manga_id), None) => Some(DeepLinkResult::Manga {
					key: manga_id.into(),
				}),
				(Some(manga_id), Some(chapter_id)) => Some(DeepLinkResult::Chapter {
					manga_key: manga_id.into(),
					key: chapter_id.into(),
				}),
				_ => None,
			},
			_ => None,
		};
		Ok(deep_link_result)
	}
}

impl ListingProvider for Dm5 {
	fn get_manga_list(&self, listing: Listing, page: i32) -> Result<MangaPageResult> {
		let url = match listing.id.as_str() {
			"popular" => format!("{}/manhua-list-p{}/", BASE_URL, page),
			"latest" => format!("{}/manhua-list-s2-p{}/", BASE_URL, page),
			"news" => format!("{}/manhua-list-s18-p{}/", BASE_URL, page),
			_ => return self.get_search_manga_list(None, page, Vec::new()),
		};

		let html = aidoku::imports::net::Request::get(url)?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-TW")
			.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
			.header("Referer", BASE_URL)
			.html()?;

		html.manga_page_result()
	}
}

register_source!(
	Dm5,
	ListingProvider,
	ImageRequestProvider,
	DeepLinkHandler
);