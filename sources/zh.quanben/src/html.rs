//! HTML parsing for quanben5: listing/search items, book details, chapter
//! list and chapter body text. Also the JSONP content-string decoder.

use aidoku::{
	Chapter, ContentRating, Manga, MangaPageResult, MangaStatus, Result,
	alloc::{String, Vec, collections::VecDeque, string::ToString as _, vec},
	imports::html::{Document, Element, Html},
	prelude::*,
};
use regex::Regex;
use spin::Lazy;

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
		.and_then(|img| img.attr("src"));

	Some(Manga {
		key: id.to_string(),
		url: Some(format!("{}/n/{}/", crate::BASE_URL, id)),
		cover,
		title,
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
			.and_then(|img| img.attr("src"));
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

// Compile the regexes once instead of per request to keep the
// chapter-list and page-list calls fast on interpreted runtimes
// (see Aidoku-Community/sources#829).
static NUMBER_RE: Lazy<Regex> = Lazy::new(|| {
	#[expect(clippy::unwrap_used)]
	Regex::new(r"\d+").unwrap()
});
static BLOCK_RE: Lazy<Regex> = Lazy::new(|| {
	#[expect(clippy::unwrap_used)]
	Regex::new(
		r"(?is)<(?:script|ins|iframe)[^>]*>.*?</(?:script|ins|iframe)>|<(?:script|ins|iframe)[^>]*/>",
	)
	.unwrap()
});
static AD_RE: Lazy<Regex> = Lazy::new(|| {
	#[expect(clippy::unwrap_used)]
	Regex::new(r#"(?is)<span[^>]*id="ad"[^>]*>.*?</span>"#).unwrap()
});
static P_RE: Lazy<Regex> = Lazy::new(|| {
	#[expect(clippy::unwrap_used)]
	Regex::new(r"(?is)<p[^>]*>(.*?)</p>").unwrap()
});
static TAGS_RE: Lazy<Regex> = Lazy::new(|| {
	#[expect(clippy::unwrap_used)]
	Regex::new(r"(?s)<[^>]+>").unwrap()
});
static END_RE: Lazy<Regex> = Lazy::new(|| {
	#[expect(clippy::unwrap_used)]
	Regex::new(r"\(本章完\)").unwrap()
});

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
		// The list is oldest-first; Aidoku expects the newest chapter
		// first, so push each entry onto the front of the deque.
		let mut chapters: VecDeque<Chapter> = VecDeque::new();
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
			let number = NUMBER_RE
				.find(&title)
				.and_then(|m| m.as_str().parse::<f32>().ok());
			chapters.push_front(Chapter {
				key: chapter.to_string(),
				title: (!title.is_empty()).then_some(title),
				chapter_number: number,
				url: Some(href),
				..Default::default()
			});
		}
		Ok(chapters.into_iter().collect())
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
	html = BLOCK_RE.replace_all(&html, "").into_owned();
	html = AD_RE.replace_all(&html, "").into_owned();

	// Paragraphs: `<p>…</p>` blocks become lines.
	let mut text = String::new();
	for cap in P_RE.captures_iter(&html) {
		let body = &cap[1];
		let line = TAGS_RE.replace_all(body, "");
		let line = line.trim();
		if !line.is_empty() {
			text.push_str(line);
			text.push('\n');
		}
	}

	// Drop the common chapter-end marker and trailing blank lines.
	let mut text = END_RE.replace_all(&text, "").into_owned();
	while text.ends_with('\n') {
		text.pop();
	}
	text
}
