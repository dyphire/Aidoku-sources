//! HTML parsing for quanben5: listing/search items, book details, chapter
//! list and chapter body text. Also the JSONP content-string decoder.

use aidoku::{
	Chapter, ContentRating, Manga, MangaPageResult, MangaStatus, Result,
	alloc::{String, Vec, string::ToString as _, vec},
	imports::html::{Document, Element, Html},
	prelude::*,
};
use regex::Regex;

/// Unescape a JSON string body the way the search JSONP stores its `content`
/// field: `\/` `\"` `\\` `\r` `\n` `\t` and `\uXXXX` (BMP chars).
pub fn decode_json_content(s: &str) -> String {
	let bytes = s.as_bytes();
	let mut out = String::new();
	let mut i = 0;
	while i < bytes.len() {
		if bytes[i] == b'\\' && i + 1 < bytes.len() {
			match bytes[i + 1] {
				b'/' => {
					out.push('/');
					i += 2;
				}
				b'"' => {
					out.push('"');
					i += 2;
				}
				b'\\' => {
					out.push('\\');
					i += 2;
				}
				b'r' | b'n' | b't' => i += 2,
				b'u' if i + 6 <= bytes.len() => {
					let hex = &s[i + 2..i + 6];
					if let Ok(cp) = u32::from_str_radix(hex, 16)
						&& let Some(ch) = char::from_u32(cp)
					{
						out.push(ch);
					} else {
						out.push('\\');
						i += 2;
						continue;
					}
					i += 6;
				}
				_ => {
					out.push(bytes[i] as char);
					i += 1;
				}
			}
		} else {
			out.push(bytes[i] as char);
			i += 1;
		}
	}
	out
}

/// Extract the decoded `content` HTML from a `search({...})` JSONP body.
/// Tolerates a trailing `;` and any whitespace after the closing paren.
pub fn extract_search_html(body: &str) -> Option<String> {
	let open = body.find("search(")?;
	let rest = &body[open + "search(".len()..];
	// The JSON object runs until the last `)` (JSONP may end with `);`).
	let end = rest.rfind(')')?;
	let obj = rest[..end].trim();
	let prefix = "\"content\":\"";
	let start = obj.find(prefix)? + prefix.len();
	// Scan for the closing quote of the content string, skipping escaped
	// `\"` (and `\\`) pairs inside the JSON string.
	let bytes = obj.as_bytes();
	let mut i = start;
	let mut escaped = false;
	let content_end = loop {
		if i >= bytes.len() {
			break None;
		}
		if bytes[i] == b'"' && !escaped {
			break Some(i);
		}
		escaped = bytes[i] == b'\\' && !escaped;
		i += 1;
	};
	Some(decode_json_content(&obj[start..content_end?]))
}

/// Trait for parsing `.pic_txt_list` items (browse listing pages share the
/// same item template as the decoded search-content HTML).
pub trait MangaPage {
	fn list_page_result(&self, page: i32) -> Result<MangaPageResult>;
}

/// Parse the encrypted JSONP search response body into a page result.
pub fn search_page_result(body: &str) -> Result<MangaPageResult> {
	let Some(content) = extract_search_html(body) else {
		bail!("search: cannot parse JSONP response");
	};
	let document = Html::parse(content)?;
	let mut result = document.list_page_result(0)?;
	// The search endpoint returns everything in one batch; a genuine
	// no-match payload parses to an empty list.
	result.has_next_page = false;
	Ok(result)
}

fn parse_item(item: &Element) -> Option<Manga> {
	let href = item.select_first("h3 a").and_then(|a| a.attr("href"))?;
	// The book id is a pinyin slug (`/n/doupocangqiong/`, some contain `-`).
	// Take the last path segment so both relative and absolute hrefs work.
	let id = href.trim_end_matches('/').rsplit('/').next()?;
	if id.is_empty()
		|| !id
			.chars()
			.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
	{
		return None;
	}
	let title = item
		.select_first("h3 a")
		.and_then(|a| a.text())
		.map(|t| t.trim().to_string())
		.unwrap_or_default();
	if title.is_empty() {
		return None;
	}
	let cover = item
		.select_first("div.pic img")
		.and_then(|img| img.attr("src"))
		.map(|u| u.to_string());
	let author = item
		.select_first("p.info")
		.and_then(|p| p.text())
		.map(|t| t.trim().to_string())
		.filter(|t| !t.is_empty())
		.map(|t| {
			t.strip_prefix("作者:")
				.map(|s| s.trim().to_string())
				.unwrap_or(t)
		})
		// Search items render the author as "天蚕土豆(完结)" — strip the
		// trailing status marker from the author name.
		.map(|a| {
			["(完结)", "(连载)", "(全本)"]
				.iter()
				.find_map(|suffix| a.strip_suffix(suffix))
				.unwrap_or(a.as_str())
				.trim()
				.to_string()
		});

	Some(Manga {
		key: id.to_string(),
		url: Some(format!("{}/n/{}/", crate::BASE_URL, id)),
		cover,
		title,
		authors: author.map(|a| vec![a]),
		// The site only hosts completed general-audience web novels.
		content_rating: ContentRating::Safe,
		..Default::default()
	})
}

impl MangaPage for Document {
	fn list_page_result(&self, _page: i32) -> Result<MangaPageResult> {
		let Some(items) = self.select(".pic_txt_list") else {
			return Ok(MangaPageResult::default());
		};
		let entries: Vec<Manga> = items.filter_map(|item| parse_item(&item)).collect();
		Ok(MangaPageResult {
			entries,
			has_next_page: crate::net::has_next_page(self),
		})
	}
}

/// Trait for parsing the book details page (`/n/{id}/`).
pub trait MangaDetails {
	fn update_details(&self, manga: &mut Manga) -> Result<()>;
}

impl MangaDetails for Document {
	fn update_details(&self, manga: &mut Manga) -> Result<()> {
		manga.url = Some(format!("{}/n/{}/", crate::BASE_URL, manga.key));
		manga.content_rating = ContentRating::Safe;
		if let Some(title) = self
			.select_first("h3")
			.and_then(|el| el.text())
			.map(|t| t.trim().to_string())
			.filter(|t| !t.is_empty())
		{
			manga.title = title;
		}
		manga.cover = self
			.select_first(".pic img")
			.and_then(|img| img.attr("src"))
			.map(|u| u.to_string());
		if let Some(desc) = self
			.select_first(".description")
			.and_then(|el| el.text())
			.map(|t| t.trim().to_string())
			.filter(|t| !t.is_empty())
		{
			manga.description = Some(desc);
		}
		// Only the *first* `.pic_txt_list` block is the book's own metadata
		// (author / category / status); the following blocks are the
		// "related books" section, whose authors must not override ours.
		let Some(info) = self.select_first(".pic_txt_list") else {
			return Ok(());
		};
		let Some(items) = info.select("p") else {
			return Ok(());
		};
		for p in items {
			let text = p.text().map(|t| t.trim().to_string()).unwrap_or_default();
			if let Some(author) = text.strip_prefix("作者:") {
				manga.authors = Some(vec![author.trim().to_string()]);
			} else if let Some(category) = text.strip_prefix("类别:") {
				manga.tags = Some(vec![category.trim().to_string()]);
			} else if let Some(status) = text.strip_prefix("状态:") {
				manga.status = if status.contains("完结") {
					MangaStatus::Completed
				} else if status.contains("连载") {
					MangaStatus::Ongoing
				} else {
					MangaStatus::Unknown
				};
			}
		}
		Ok(())
	}
}

/// Trait for parsing the chapter list (`/n/{id}/xiaoshuo.html`).
pub trait ChapterPage {
	/// Chapters are returned newest-first, as CONTRIBUTING requires.
	fn chapters(&self) -> Result<Vec<Chapter>>;
}

impl ChapterPage for Document {
	fn chapters(&self) -> Result<Vec<Chapter>> {
		let Some(links) = self.select("ul.list > li > a") else {
			return Ok(Vec::new());
		};
		let number_re = Regex::new(r"\d+").ok();
		let mut chapters: Vec<Chapter> = Vec::new();
		for link in links {
			let Some(href) = link.attr("href") else {
				continue;
			};
			let id = href.trim_end_matches(".html");
			let Some(chapter) = id.rsplit('/').next() else {
				continue;
			};
			if chapter.is_empty() || !chapter.chars().all(|c| c.is_ascii_digit()) {
				continue;
			}
			let title = link
				.text()
				.map(|t| t.trim().to_string())
				.unwrap_or_default();
			let number = number_re
				.as_ref()
				.and_then(|re| re.find(&title))
				.and_then(|m| m.as_str().parse::<f32>().ok());
			chapters.push(Chapter {
				key: chapter.to_string(),
				title: (!title.is_empty()).then_some(title),
				chapter_number: number,
				url: Some(href),
				..Default::default()
			});
		}
		chapters.reverse();
		Ok(chapters)
	}
}

/// Extract the body text of a chapter: every `<p>` in `#content` becomes a
/// paragraph; empty ad placeholders (`<span id="ad"></span>`) and the
/// trailing `(本章完)` marker are dropped.
pub fn chapter_text(document: &Document) -> String {
	let Some(content) = document.select_first("#content") else {
		return String::new();
	};
	let mut html = content.html().unwrap_or_default();

	// Drop ad spans and any script/iframe blocks first.
	let re_block = Regex::new(
		r"(?is)<(?:script|ins|iframe)[^>]*>.*?</(?:script|ins|iframe)>|<(?:script|ins|iframe)[^>]*/>",
	)
	.ok();
	if let Some(re) = re_block {
		html = re.replace_all(&html, "").into_owned();
	}
	let re_ad = Regex::new(r#"(?is)<span[^>]*id="ad"[^>]*>.*?</span>"#).ok();
	if let Some(re) = re_ad {
		html = re.replace_all(&html, "").into_owned();
	}

	// Paragraphs: `<p>…</p>` blocks become lines.
	let re_p = Regex::new(r"(?is)<p[^>]*>(.*?)</p>").ok();
	let re_tags = Regex::new(r"(?s)<[^>]+>").ok();
	let mut text = String::new();
	if let Some(re) = re_p {
		for cap in re.captures_iter(&html) {
			let body = &cap[1];
			let line = match &re_tags {
				Some(tags) => tags.replace_all(body, "").into_owned(),
				None => body.to_string(),
			};
			let line = line.trim().to_string();
			if !line.is_empty() {
				text.push_str(&line);
				text.push('\n');
			}
		}
	}

	// Drop the common chapter-end marker and trailing blank lines.
	let re_end = Regex::new(r"\(本章完\)").ok();
	let mut text = if let Some(re) = re_end {
		re.replace_all(&text, "").into_owned()
	} else {
		text
	};
	while text.ends_with('\n') {
		text.pop();
	}
	text
}
