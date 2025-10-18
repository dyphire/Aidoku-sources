use crate::{handle_cover_url, BASE_URL};
use aidoku::{
	alloc::{string::ToString as _, vec, String, Vec},
	error,
	imports::{
		html::{Document, ElementList},
		net::{HttpMethod, Request},
	},
	prelude::*,
	Manga, MangaPageResult, MangaStatus, Result,
};

pub trait MangaPage {
	fn update_details(&self, manga: &mut Manga) -> Result<()>;
	fn manga_page_result(&self) -> Result<MangaPageResult>;
}

impl MangaPage for Document {
	fn update_details(&self, manga: &mut Manga) -> Result<()> {
		let ids = manga.key.split("/").collect::<Vec<&str>>();
		let url = format!("{}/manga/{}", BASE_URL, ids[0]);
		let html = Request::new(url.clone(), HttpMethod::Get)?
			.header("Origin", BASE_URL)
			.html()?;

		// Try to parse JSON-LD structured data first
		if let Some(obj) = html
			.select("script[type='application/ld+json']")
			.and_then(|scripts| scripts.first())
			.and_then(|script| script.text())
			.and_then(|json_text| serde_json::from_str::<serde_json::Value>(&json_text).ok())
			.and_then(|json| json.as_object().cloned())
		{
			// Extract data from JSON-LD
			manga.title = obj
				.get("name")
				.and_then(|v| v.as_str())
				.unwrap_or("Unknown")
				.to_string();
			manga.description = obj
				.get("description")
				.and_then(|v| v.as_str())
				.map(|s| s.to_string());
			manga.cover = obj
				.get("image")
				.and_then(|v| v.as_str())
				.map(|s| handle_cover_url(s.to_string()));

			// Extract authors
			if let Some(authors) = obj.get("author").and_then(|v| v.as_array()) {
				let author_names: Vec<String> = authors
					.iter()
					.filter_map(|author| author.as_object())
					.filter_map(|author_obj| author_obj.get("name"))
					.filter_map(|name| name.as_str())
					.map(|s| s.to_string())
					.collect();
				if !author_names.is_empty() {
					manga.authors = Some(author_names);
				}
			}

			// Extract genres/tags
			if let Some(genres) = obj.get("genre").and_then(|v| v.as_array()) {
				let tags: Vec<String> = genres
					.iter()
					.filter_map(|genre| genre.as_str())
					.map(|s| s.to_string())
					.collect();
				if !tags.is_empty() {
					manga.tags = Some(tags);
				}
			}

			// Extract status
			if let Some(status) = obj.get("creativeWorkStatus").and_then(|v| v.as_str()) {
				manga.status = match status {
					"連載中" => MangaStatus::Ongoing,
					"完結" | "已完結" => MangaStatus::Completed,
					_ => MangaStatus::Unknown,
				};
			}

			// Get mid from HTML (still needed for API calls)
			let mid = html
				.select("#mangachapters")
				.and_then(|e| e.first())
				.and_then(|e| e.attr("data-mid"))
				.unwrap_or_else(String::new);
			manga.key = format!("{}/{}", ids[0], mid);
			manga.url = Some(url);
			return Ok(());
		}

		// Fallback to HTML parsing if JSON-LD parsing fails
		let mid = html
			.select("#mangachapters")
			.and_then(|e| e.first())
			.and_then(|e| e.attr("data-mid"))
			.unwrap_or_else(String::new);
		manga.cover = html
			.select("meta[property='og:image']")
			.and_then(|e| e.first())
			.and_then(|e| e.attr("content"))
			.map(handle_cover_url);
		manga.title = html
			.select("title")
			.and_then(|e| e.first())
			.and_then(|e| e.text())
			.map(|t| t.replace("-G站漫畫", ""))
			.unwrap_or_else(String::new);
		let author = html
			.select("a[href*=author]>span")
			.map(|elements| {
				elements
					.filter_map(|a| a.text().map(|t| t.replace(",", "")))
					.filter(|a| !a.is_empty())
					.collect::<Vec<String>>()
					.join(", ")
			})
			.unwrap_or_else(String::new);
		let description = html
			.select(".text-medium.my-unit-md")
			.and_then(|e| e.first())
			.and_then(|e| e.text())
			.unwrap_or_else(String::new);
		let categories = html
			.select(".py-1>a:not([href*=author])>span")
			.map(|elements| {
				elements
					.filter_map(|a| {
						a.text().map(|t| {
							t.replace(",", "")
								.replace("热门漫画", "")
								.replace("#", "")
								.replace("热门推荐", "")
								.trim()
								.to_string()
						})
					})
					.filter(|a| !a.is_empty())
					.collect::<Vec<String>>()
			})
			.unwrap_or_else(Vec::new);
		let status = MangaStatus::Ongoing;

		manga.key = format!("{}/{}", ids[0], mid);
		manga.authors = Some(vec![author]);
		manga.description = Some(description);
		manga.tags = Some(categories);
		manga.status = status;
		manga.url = Some(url);

		Ok(())
	}

	fn manga_page_result(&self) -> Result<MangaPageResult> {
		let mut entries: Vec<Manga> = Vec::new();

		for item in self.try_select(".pb-2>a")? {
			let href = item.attr("href").unwrap_or_else(String::new);
			let id = href
				.split("/")
				.filter(|s| !s.is_empty())
				.last()
				.unwrap_or("")
				.to_string();
			let cover = handle_cover_url(
				item.select("div>img")
					.and_then(|img| img.first())
					.and_then(|img| img.attr("src"))
					.unwrap_or_else(String::new),
			);
			let title = item
				.select("div>h3")
				.and_then(|h3| h3.first())
				.and_then(|h3| h3.text())
				.unwrap_or_else(String::new);

			if !id.is_empty() && !title.is_empty() {
				entries.push(Manga {
					key: id,
					cover: Some(cover),
					title,
					..Default::default()
				});
			}
		}

		let has_next_page = self
			.select("a[href*='/page/'][aria-label='下一頁']")
			.map(|list| list.first().is_some())
			.unwrap_or(false);

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
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
