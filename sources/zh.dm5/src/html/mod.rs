use crate::{BASE_URL, USER_AGENT};
use aidoku::{
	alloc::{string::ToString as _, vec, String, Vec},
	error,
	imports::{
		html::{Document, Element, ElementList},
		net::HttpMethod,
	},
	prelude::*,
	Chapter, Manga, MangaPageResult, MangaStatus, Page, Result,
};

pub trait MangaPage {
	fn update_details(&self, manga: &mut Manga) -> Result<()>;
	fn manga_page_result(&self) -> Result<MangaPageResult>;
}

impl MangaPage for Document {
	fn update_details(&self, manga: &mut Manga) -> Result<()> {
		let title = self
			.select("div.banner_detail_form p.title")
			.and_then(|e| e.first())
			.and_then(|e| e.own_text())
			.unwrap_or_else(|| String::new());
		let cover = self
			.select("div.banner_detail_form img")
			.and_then(|e| e.first())
			.and_then(|e| e.attr("abs:src"))
			.unwrap_or_else(|| String::new());
		let author = self
			.select("div.banner_detail_form p.subtitle > a")
			.and_then(|e| e.first())
			.and_then(|e| e.text())
			.unwrap_or_else(|| String::new());
		let genres = self
			.select("div.banner_detail_form p.tip a")
			.map(|elements| {
				elements
					.filter_map(|a| a.text())
					.collect::<Vec<String>>()
					.join(", ")
			})
			.unwrap_or_else(|| String::new());
		let el = self
			.select("div.banner_detail_form p.content")
			.and_then(|e| e.first())
			.unwrap();
		let mut description = el.own_text().unwrap_or_default();
		if let Some(span) = el.select("span").and_then(|s| s.first()) {
			if let Some(span_text) = span.own_text() {
				description.push_str(&span_text);
			}
		}
		let status_text = self
			.select("div.banner_detail_form p.tip > span > span")
			.and_then(|e| e.first())
			.and_then(|e| e.text())
			.unwrap_or_else(|| String::new());
		let status = match status_text.as_str() {
			"连载中" => MangaStatus::Ongoing,
			"已完结" => MangaStatus::Completed,
			_ => MangaStatus::Unknown,
		};

		manga.title = title;
		manga.cover = Some(cover);
		manga.authors = Some(vec![author]);
		manga.tags = Some(vec![genres]);
		manga.description = Some(description);
		manga.status = status;
		manga.url = Some(format!("{}/{}", BASE_URL, manga.key));

		Ok(())
	}

	fn manga_page_result(&self) -> Result<MangaPageResult> {
		let mut entries: Vec<Manga> = Vec::new();

		// Handle banner_detail_form for search
		if let Some(banner) = self.select("div.banner_detail_form").and_then(|e| e.first()) {
			let title = banner.select("p.title").and_then(|e| e.first()).and_then(|e| e.own_text()).unwrap_or_default();
			let cover = banner.select("img").and_then(|e| e.first()).and_then(|e| e.attr("abs:src")).unwrap_or_default();
			let url = banner.select("p.title").and_then(|e| e.first()).and_then(|e| e.attr("href")).unwrap_or_default();
			let id = url.split('/').last().unwrap_or("").to_string();
			if !id.is_empty() && !title.is_empty() {
				entries.push(Manga {
					key: id,
					cover: Some(cover),
					title,
					..Default::default()
				});
			}
		}

		// Handle list items
		let list_items = self.select("ul.mh-list li");
		if let Some(items) = list_items {
			if items.is_empty() {
				return Err(aidoku::error!("Debug: ul.mh-list li found but empty"));
			}
			for item in items {
				let title = item.select("a").and_then(|e| e.first()).and_then(|e| e.text()).unwrap_or_default();
				let url = item.select("a").and_then(|e| e.first()).and_then(|e| e.attr("href")).unwrap_or_default();
				let id = url.split('/').filter(|s| !s.is_empty()).last().unwrap_or("").to_string();
				let cover_style = item.select("p.mh-cover").and_then(|e| e.first()).and_then(|e| e.attr("style")).unwrap_or_default();
				let cover = cover_style.split("url(").nth(1).and_then(|s| s.split(')').next()).map(|s| s.to_string()).unwrap_or_default();

				if !id.is_empty() && !title.is_empty() {
					entries.push(Manga {
						key: id,
						cover: Some(cover),
						title,
						..Default::default()
					});
				}
			}
		} else {
			return Err(aidoku::error!("Debug: No ul.mh-list > li > div.mh-item found"));
		}

		let has_next_page = self
			.select("div.page-pagination a:contains(>)")
			.map(|list| list.first().is_some())
			.unwrap_or(false);

		if entries.is_empty() {
			return Err(aidoku::error!("Debug: No entries found after parsing"));
		}

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}
}

pub struct ChapterList;

impl ChapterList {
	pub fn get_chapters(manga_id: &str) -> Result<Vec<Chapter>> {
		let url = format!("{}/{}", BASE_URL, manga_id);
		let html = aidoku::imports::net::Request::new(url, HttpMethod::Get)?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-TW")
			.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
			.header("Referer", BASE_URL)
			.header("DNT", "1")
			.html()?;
		let container = html
			.select("div#chapterlistload")
			.and_then(|e| e.first())
			.ok_or_else(|| aidoku::error!("Chapter list not found"))?;
		let li = container.select("li > a");
		let mut chapters: Vec<Chapter> = Vec::new();

		if let Some(li) = li {
			let mut li_vec: Vec<Element> = li.into_iter().collect();
			li_vec.reverse();
			for (index, item) in li_vec.into_iter().enumerate() {
				let href = item.attr("href").unwrap_or_default();
				let title = item.select("p.title").and_then(|e| e.first()).and_then(|e| e.text()).unwrap_or_else(|| item.text().unwrap_or_default());
				let locked = item.select("span.detail-lock, span.view-lock").and_then(|e| e.first()).is_some();

				chapters.push(Chapter {
					key: href.to_string(),
					title: Some(title),
					chapter_number: Some((index + 1) as f32),
					url: Some(format!("{}{}", BASE_URL, href)),
					locked,
					..Default::default()
				});
			}
		}

		chapters.reverse();

		Ok(chapters)
	}
}

pub struct PageList;

impl PageList {
	pub fn get_pages(_manga_id: String, chapter_id: String) -> Result<Vec<Page>> {
		let url = format!("{}{}", BASE_URL, chapter_id);
		let html = aidoku::imports::net::Request::new(url, HttpMethod::Get)?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-TW")
			.header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
			.header("Referer", BASE_URL)
			.header("DNT", "1")
			.html()?;
		let images = html.select("div#barChapter > img.load-src");
		let mut pages: Vec<Page> = Vec::new();

		if let Some(img_list) = images {
			if !img_list.is_empty() {
				for (_index, img) in img_list.into_iter().enumerate() {
					let data_src = img.attr("data-src").unwrap_or_default();
					pages.push(Page {
						content: aidoku::PageContent::Url(data_src, None),
						..Default::default()
					});
				}
			}
		} else {
			// Handle packed images
			let script = html
				.select("script:contains(DM5_MID)")
				.and_then(|e| e.first())
				.and_then(|e| e.text())
				.ok_or_else(|| aidoku::error!("Script not found"))?;
			if !script.contains("DM5_VIEWSIGN_DT") {
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
				.and_then(|s| s.parse().ok())
				.ok_or_else(|| aidoku::error!("Image count not found"))?;

			for i in 1..=image_count {
				let page_url = format!(
					"{}/chapterfun.ashx?cid={}&page={}&key=&language=1&gtk=6&_cid={}&_mid={}&_dt={}&_sign={}",
					BASE_URL, cid, i, cid, mid, dt, sign
				);
				pages.push(Page {
					content: aidoku::PageContent::Url(page_url, None),
					..Default::default()
				});
			}
		}

		Ok(pages)
	}
}

trait TrySelect {
	fn try_select<S: AsRef<str>>(&self, css_query: S) -> Result<ElementList>;
}

impl TrySelect for Document {
	fn try_select<S: AsRef<str>>(&self, css_query: S) -> Result<ElementList> {
		self.select(&css_query)
			.ok_or_else(|| error!("No element found for selector: `{}`", css_query.as_ref()))
	}
}
