use crate::{BASE_URL, USER_AGENT, net::Url};
use aidoku::{
	Manga, MangaPageResult, MangaStatus, Page, Result,
	alloc::{String, Vec, string::ToString as _},
	error,
	imports::html::{Document, Element, ElementList},
	prelude::*,
};
use regex::Regex;

fn extract_chapter_number(title: &str) -> Option<f32> {
	let re = Regex::new(r"(?:第\s*)([\d０-９]+(?:\.[\d０-９]+)?)|([\d０-９]+(?:\.[\d０-９]+)?)\s*(?:话|話|章|回|卷|册|冊)").unwrap();
	if let Some(captures) = re.captures(title) {
		let num_match = captures.get(1).or_else(|| captures.get(2));
		if let Some(num_match) = num_match {
			let num_str = num_match
				.as_str()
				.chars()
				.map(|c| match c {
					'０' => '0',
					'１' => '1',
					'２' => '2',
					'３' => '3',
					'４' => '4',
					'５' => '5',
					'６' => '6',
					'７' => '7',
					'８' => '8',
					'９' => '9',
					'．' => '.',
					other => other,
				})
				.collect::<String>();
			if let Ok(num) = num_str.parse::<f32>() {
				return Some(num);
			}
		}
	}
	None
}

pub trait MangaPage {
	fn update_details(&self, manga: &mut Manga) -> Result<()>;
	fn manga_page_result(&self) -> Result<MangaPageResult>;
}

impl MangaPage for Document {
	fn update_details(&self, manga: &mut Manga) -> Result<()> {
		manga.cover = self.try_select_first(".book-cover")?.attr("src");
		manga.title = self
			.try_select_first("h1.book-title")?
			.text()
			.unwrap_or_default();
		let authors = self
			.try_select(".authorname,.illname")?
			.filter_map(|a| a.text())
			.collect::<Vec<String>>();
		manga.authors = Some(authors);
		manga.description = Some(
			self.try_select_first(".book-summary>content")?
				.text()
				.unwrap_or_default(),
		);
		let tags = self
			.try_select(".tag-small-group>.tag-small>a")?
			.filter_map(|a| a.text())
			.collect::<Vec<String>>();
		manga.tags = Some(tags);
		manga.status = match self
			.try_select_first(".book-layout-inline")?
			.text()
			.unwrap_or_default()
			.trim()
			.split("|")
			.map(|a| a.trim().to_string())
			.collect::<Vec<String>>()
			.first()
			.unwrap()
			.as_str()
		{
			"連載" => MangaStatus::Ongoing,
			"完結" => MangaStatus::Completed,
			_ => MangaStatus::Unknown,
		};
		manga.url = Some(Url::manga(manga.key.clone()).to_string());
		Ok(())
	}
	fn manga_page_result(&self) -> Result<MangaPageResult> {
		let mut entries: Vec<Manga> = Vec::new();

		// Parse manga list
		let items = self.try_select(".book-li>a")?;
		for item in items {
			let href = item.attr("href").unwrap_or_default();
			let key = href
				.split("/")
				.filter_map(|s| if s.is_empty() { None } else { Some(s) })
				.last()
				.unwrap_or_default()
				.replace(".html", "");

			let cover = item
				.select(".book-cover>img")
				.and_then(|img| img.first())
				.and_then(|img| img.attr("data-src"));
			let title = item
				.select(".book-title")
				.and_then(|title| title.text())
				.unwrap_or_default();

			if !key.is_empty() && !title.is_empty() {
				entries.push(Manga {
					key,
					cover,
					title,
					..Default::default()
				});
			}
		}

		let has_next_page = self
			.select("#pagelink")
			.and_then(|pagelink| {
				let strong_text = pagelink
					.select("strong")
					.and_then(|s| s.first())
					.and_then(|s| s.text());
				let last_text = pagelink
					.select(".last")
					.and_then(|l| l.first())
					.and_then(|l| l.text());
				if let (Some(current), Some(last)) = (strong_text, last_text) {
					Some(current != last)
				} else {
					pagelink
						.select(".next")
						.and_then(|n| n.first())
						.and_then(|n| n.attr("href"))
						.map(|href| href != "#")
				}
			})
			.unwrap_or(false);

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}
}

pub trait ChapterPage {
	fn chapters(&self, manga_id: &str) -> Result<Vec<aidoku::Chapter>>;
}

impl ChapterPage for Document {
	fn chapters(&self, _manga_id: &str) -> Result<Vec<aidoku::Chapter>> {
		let volumes = self.try_select(".catalog-volume")?;
		let mut chapters: Vec<aidoku::Chapter> = Vec::new();

		for volume in volumes {
			let volume_title = volume.select("h3").unwrap().text().unwrap_or_default();
			let volume_num = extract_chapter_number(&volume_title).unwrap_or(-1.0);
			let chapter_links = volume.select(".chapter-li-a").unwrap();

			if !chapter_links.is_empty() {
				let mut has_javascript_link = false;
				for link in chapter_links {
					if link
						.attr("href")
						.unwrap_or_default()
						.starts_with("javascript:")
					{
						has_javascript_link = true;
						break;
					}
				}
				let volume_thumbnail = volume
					.select(".volume-cover-img img")
					.and_then(|img| img.first())
					.and_then(|img| img.attr("data-src"));

				if has_javascript_link {
					let vol_href = volume
						.select(".volume-cover-img")
						.unwrap()
						.first()
						.unwrap()
						.attr("href")
						.unwrap_or_default();
					let vol_url = format!("{}{}", BASE_URL, vol_href);
					let vol_html = aidoku::imports::net::Request::get(vol_url)?
						.header("User-Agent", USER_AGENT)
						.header("Origin", BASE_URL)
						.html()?;
					for chapter_item in vol_html.try_select(".catalog-volume .chapter-li-a")? {
						let chapter_href = chapter_item.attr("href").unwrap_or_default();
						let chapter_key = chapter_href
							.split("/")
							.map(|a| a.to_string())
							.filter(|a| !a.is_empty())
							.collect::<Vec<String>>()
							.pop()
							.unwrap()
							.replace(".html", "");
						let title = chapter_item
							.select("span")
							.unwrap()
							.text()
							.unwrap_or_default();
						let chapter_num =
							extract_chapter_number(&title).unwrap_or(chapters.len() as f32 + 1.0);
						let url = format!("{}{}", BASE_URL, chapter_href);
						chapters.push(aidoku::Chapter {
							key: chapter_key,
							title: Some(title),
							volume_number: Some(volume_num),
							chapter_number: Some(chapter_num),
							url: Some(url),
							thumbnail: volume_thumbnail.clone(),
							..Default::default()
						});
					}
				} else {
					let chapter_links = volume.select(".chapter-li-a").unwrap();
					for item in chapter_links {
						let chapter_href = item.attr("href").unwrap_or_default();
						let chapter_key = chapter_href
							.split("/")
							.map(|a| a.to_string())
							.filter(|a| !a.is_empty())
							.collect::<Vec<String>>()
							.pop()
							.unwrap()
							.replace(".html", "");
						let title = item.select("span").unwrap().text().unwrap_or_default();
						let chapter_num =
							extract_chapter_number(&title).unwrap_or(chapters.len() as f32 + 1.0);
						let url = format!("{}{}", BASE_URL, chapter_href);
						chapters.push(aidoku::Chapter {
							key: chapter_key,
							title: Some(title),
							chapter_number: Some(chapter_num),
							volume_number: Some(volume_num),
							url: Some(url),
							thumbnail: volume_thumbnail.clone(),
							..Default::default()
						});
					}
				}
			}
		}
		chapters.reverse();
		Ok(chapters)
	}
}

pub trait PageList {
	fn pages(&self) -> Result<Vec<Page>>;
}

impl PageList for Document {
	fn pages(&self) -> Result<Vec<Page>> {
		let mut pages: Vec<Page> = Vec::new();
		for item in self.try_select("#acontentz>img")? {
			let url = item.attr("data-src").unwrap_or_default().trim().to_string();
			pages.push(Page {
				content: aidoku::PageContent::Url(url, None),
				..Default::default()
			});
		}
		Ok(pages)
	}
}

trait TrySelect {
	fn try_select<S: AsRef<str>>(&self, css_query: S) -> Result<ElementList>;
	fn try_select_first<S: AsRef<str>>(&self, css_query: S) -> Result<Element>;
}

impl TrySelect for Document {
	fn try_select<S: AsRef<str>>(&self, css_query: S) -> Result<ElementList> {
		self.select(&css_query)
			.ok_or_else(|| error!("No element found for selector: `{}`", css_query.as_ref()))
	}

	fn try_select_first<S: AsRef<str>>(&self, css_query: S) -> Result<Element> {
		self.select_first(&css_query)
			.ok_or_else(|| error!("No element found for selector: `{}`", css_query.as_ref()))
	}
}
