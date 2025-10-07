use crate::net::Url;
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
			let num_str = num_match.as_str()
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
		manga.title = self.try_select_first("h1.book-title")?.text().unwrap_or_default();
		let authors = self
			.try_select(".authorname,.illname")?
			.filter_map(|a| a.text())
			.collect::<Vec<String>>();
		manga.authors = Some(authors);
		manga.description = Some(self.try_select_first(".book-summary>content")?.text().unwrap_or_default());
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
		let link = self.try_select("#pagelink")?;
		let has_next_page = if let Some(alternate) = self.select("link[rel='alternate']") {
			if alternate.first().unwrap().attr("href").unwrap_or_default().is_empty() {
				if self.try_select(".book-li>a")?.is_empty() {
					false
				} else {
					link.select("strong").unwrap().text().unwrap_or_default() != link.select(".last").unwrap().text().unwrap_or_default()
				}
			} else {
				link.select(".next").unwrap().first().unwrap().attr("href").unwrap_or_default() != "#"
			}
		} else {
			false
		};

		let mut entries: Vec<Manga> = Vec::new();

		if let Some(alternate) = self.select("link[rel='alternate']") {
			if !alternate.first().unwrap().attr("href").unwrap_or_default().is_empty() {
				let alternate_url = alternate.first().unwrap().attr("href").unwrap();
				let key = alternate_url
					.split("/")
					.map(|a| a.to_string())
					.filter(|a| !a.is_empty())
					.collect::<Vec<String>>()
					.pop()
					.unwrap()
					.replace(".html", "");
				let cover = self.try_select_first(".book-cover")?.attr("src");
				let title = self.try_select_first("h1.book-title")?.text().unwrap_or_default();

				entries.push(Manga {
					key,
					cover,
					title,
					..Default::default()
				});
			}
		} else {
			for item in self.try_select(".book-li>a")? {
				let key = item
					.attr("href")
					.unwrap_or_default()
					.split("/")
					.map(|a| a.to_string())
					.filter(|a| !a.is_empty())
					.collect::<Vec<String>>()
					.pop()
					.unwrap()
					.replace(".html", "");
				let cover = item.select(".book-cover>img").unwrap().first().unwrap().attr("data-src");
				let title = item.select(".book-title").unwrap().text().unwrap_or_default();
				entries.push(Manga {
					key,
					cover,
					title,
					..Default::default()
				});
			}
		}

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
	fn chapters(&self, manga_id: &str) -> Result<Vec<aidoku::Chapter>> {
		let volumes = self.try_select(".catalog-volume")?;
		let mut chapters: Vec<aidoku::Chapter> = Vec::new();

		for volume in volumes {
			let volume_title = volume.select("h3").unwrap().text().unwrap_or_default();
			let volume_num = extract_chapter_number(&volume_title).unwrap_or(-1.0);
			let chapter_links = volume.select(".chapter-li-a").unwrap();

			if !chapter_links.is_empty() {
				let mut has_javascript_link = false;
				for link in chapter_links {
					if link.attr("href").unwrap_or_default().starts_with("javascript:") {
						has_javascript_link = true;
						break;
					}
				}
				if has_javascript_link {
					let vol_href = volume.select(".volume-cover-img").unwrap().first().unwrap().attr("href").unwrap_or_default();
					let vol_url = format!("https://www.bilimanga.net{}", vol_href);
					let vol_html = aidoku::imports::net::Request::get(vol_url)?
						.header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1")
						.header("Origin", "https://www.bilimanga.net")
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
						let title = chapter_item.select("span").unwrap().text().unwrap_or_default();
						let chapter_num = extract_chapter_number(&title).unwrap_or(chapters.len() as f32 + 1.0);
						let url = format!("https://www.bilimanga.net{}", chapter_href);
						chapters.push(aidoku::Chapter {
							key: chapter_key,
							title: Some(title),
							volume_number: Some(volume_num),
							chapter_number: Some(chapter_num),
							url: Some(url),
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
						let chapter_num = extract_chapter_number(&title).unwrap_or(chapters.len() as f32 + 1.0);
						let url = format!("https://www.bilimanga.net{}", chapter_href);
						chapters.push(aidoku::Chapter {
							key: chapter_key,
							title: Some(title),
							chapter_number: Some(chapter_num),
							volume_number: Some(volume_num),
							url: Some(url),
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
