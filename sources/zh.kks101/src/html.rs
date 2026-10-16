//! Parsers for 101kks.com pages, as extension traits on the Aidoku HTML
//! types, plus the chapter-content text converter.

use crate::net::Url;
use aidoku::{
	Chapter, Manga, MangaPageResult, MangaStatus, Result,
	alloc::{String, Vec, string::ToString, vec},
	imports::html::{Document, Element},
	prelude::*,
};
use regex::Regex;

/// Whether a response is a Cloudflare challenge page instead of real
/// content. Aidoku's built-in handler transparently solves some challenge
/// forms; when it cannot, the source must not try to parse the page.
pub fn is_challenge_page(html: &str) -> bool {
	html.contains("Just a moment")
		|| html.contains("_cf_chl_opt")
		|| html.contains("challenge-error")
}

/// The numeric id of the last URL segment, e.g. `/novels/newhot_0_0_2.html`
/// and `/search/火影/2.html` both yield 2.
pub fn page_of_href(href: &str) -> Option<i32> {
	let last = href.trim_end_matches(".html").rsplit('/').next()?;
	last.rsplit('_').next()?.parse().ok()
}

/// Extract the trailing numeric id from a `/book/{id}.html` or
/// `/txt/{book}/{chapter}.html` href (whatever segment it appears in).
fn extract_id(href: &str) -> String {
	href.rsplit('/')
		.next()
		.unwrap_or("")
		.trim_end_matches(".html")
		.split('_')
		.next()
		.unwrap_or("")
		.to_string()
}

pub trait MangaPage {
	fn manga_page_result(&self, page: i32) -> Result<MangaPageResult>;
	fn update_details(&self, manga: &mut Manga) -> Result<()>;
	/// Build a single series entry from a book detail page. Used when a
	/// search uniquely matches one book and the site 302-redirects straight
	/// to its page instead of returning a result list.
	fn manga_from_book_page(&self, key: String) -> Option<Manga>;
}

impl MangaPage for Document {
	/// Parse a book-list page (search results, ranking and category listings
	/// all use `#article_list_content li` items with `.newnav` bodies).
	fn manga_page_result(&self, page: i32) -> Result<MangaPageResult> {
		let items = if let Some(list) = self.select("#article_list_content li") {
			list
		} else {
			let Some(list) = self.select("li") else {
				return Ok(MangaPageResult {
					entries: Vec::new(),
					has_next_page: false,
				});
			};
			list
		};

		let mut entries: Vec<Manga> = Vec::new();
		for item in items {
			if let Some(manga) = parse_book_item(&item) {
				entries.push(manga);
			}
		}

		let has_next_page = self
			.select_first("#pagelink")
			.and_then(|p| p.select_first("a.next"))
			.and_then(|a| a.attr("href"))
			.and_then(|href| page_of_href(&href))
			.is_some_and(|next| next > page);

		Ok(MangaPageResult {
			entries,
			has_next_page,
		})
	}

	fn update_details(&self, manga: &mut Manga) -> Result<()> {
		manga.title = self
			.select_first(".booknav2 h1")
			.and_then(|el| el.text())
			.map(|t| t.trim().to_string())
			.filter(|t| !t.is_empty())
			.unwrap_or_default();
		manga.cover = self
			.select_first(".bookimg2 img")
			.and_then(|img| img.attr("src").or_else(|| img.attr("data-src")));

		manga.description = self
			.select_first("#tab_info .navtxt p")
			.and_then(|el| el.text())
			.map(|t| t.trim().to_string())
			.filter(|t| !t.is_empty());

		// Author links are served as absolute URLs
		// (`https://101kks.com/author/…`), so match on substring. The site
		// shows every detail-page author as "佚名"; when a real author was
		// already captured from a list page, keep it instead.
		let parsed_authors: Vec<String> = self
			.select(".booknav2 p a[href*=\"/author/\"]")
			.map(|els| els.filter_map(|a| a.text()).collect())
			.unwrap_or_default();
		if !parsed_authors.is_empty() {
			let is_placeholder = parsed_authors[0].trim() == "佚名";
			if !is_placeholder || manga.authors.as_ref().is_none_or(|a| a.is_empty()) {
				manga.authors = Some(parsed_authors);
			}
		}

		// e.g. `<p>27.55萬字 | 連載</p>` and a category link in the same
		// block (`分類：<a ...>`). Classify from the og meta first, falling
		// back to the plain-text status marker.
		manga.status = self
			.select_first("meta[property=\"og:novel:status\"]")
			.and_then(|el| el.attr("content"))
			.or_else(|| {
				self.select(".booknav2 p")
					.map(|els| els.filter_map(|el| el.text()).collect::<Vec<_>>().join(" "))
			})
			.map(|status| {
				if status.contains("完結") || status.contains("完结") || status.contains("全本")
				{
					MangaStatus::Completed
				} else if status.contains("連載") || status.contains("连载") {
					MangaStatus::Ongoing
				} else {
					MangaStatus::Unknown
				}
			})
			.unwrap_or(MangaStatus::Unknown);

		let mut tags: Vec<String> = self
			.select(".booknav2 p a[href^=\"/novels/class/\"]")
			.map(|els| els.filter_map(|a| a.text()).collect())
			.unwrap_or_default();
		tags.truncate(1);
		manga.tags = (!tags.is_empty()).then_some(tags);

		manga.url = Some(Url::manga(&manga.key).to_string());
		Ok(())
	}

	fn manga_from_book_page(&self, key: String) -> Option<Manga> {
		let title = self
			.select_first(".booknav2 h1")
			.and_then(|el| el.text())
			.map(|t| t.trim().to_string())
			.filter(|t| !t.is_empty())?;
		let cover = self
			.select_first(".bookimg2 img")
			.and_then(|img| img.attr("src").or_else(|| img.attr("data-src")));
		Some(Manga {
			key,
			cover,
			title,
			..Default::default()
		})
	}
}

/// Parse one `#article_list_content li` item into a series entry.
fn parse_book_item(item: &Element) -> Option<Manga> {
	let href = item
		.select_first(".newnav h3 a")
		.and_then(|a| a.attr("href"))?;
	if !href.contains("/book/") {
		return None;
	}
	let key = extract_id(&href);
	let title = item
		.select_first(".newnav h3 a")
		.and_then(|a| a.text())
		.map(|t| t.trim().to_string())
		.unwrap_or_default();
	let cover = item
		.select_first("a.imgbox img")
		.and_then(|img| img.attr("data-src").or_else(|| img.attr("src")));
	// The first `.labelbox label` is the author; the detail page shows
	// every author as "佚名", so keep the list-page author instead.
	let authors = item
		.select_first(".newnav .labelbox label")
		.and_then(|el| el.text())
		.map(|t| t.trim().to_string())
		.filter(|t| !t.is_empty())
		.map(|a| vec![a]);

	if key.is_empty() || title.is_empty() {
		return None;
	}
	Some(Manga {
		key,
		cover,
		title,
		authors,
		..Default::default()
	})
}

pub trait ChapterPage {
	/// Parse a full chapter list into a newest-first `Vec<Chapter>`.
	/// The site lists chapters oldest-first, and Aidoku expects the newest
	/// chapter first.
	fn chapters(&self) -> Result<Vec<Chapter>>;
}

impl ChapterPage for Document {
	fn chapters(&self) -> Result<Vec<Chapter>> {
		let Some(links) = self
			.select("li > a[href*=\"/txt/\"]")
			.or_else(|| self.select("li a[href*=\"/txt/\"]"))
		else {
			return Ok(Vec::new());
		};

		let mut chapters: Vec<Chapter> = Vec::new();
		for link in links {
			let Some(href) = link.attr("href") else {
				continue;
			};
			if !href.contains("/txt/") {
				continue;
			}
			let key = extract_id(&href);
			if key.is_empty() {
				continue;
			}
			let title = link
				.text()
				.map(|t| t.trim().to_string())
				.unwrap_or_default();
			let number = link
				.parent()
				.and_then(|li| li.attr("data-num"))
				.and_then(|n| n.parse::<f32>().ok());
			chapters.push(Chapter {
				key,
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

/// Convert the raw `#txtcontent` HTML into Aidoku text (Markdown-ish
/// paragraphs). Ad blocks and scripts are dropped; `<br>` separates
/// paragraphs, exactly as the site serves them.
pub fn chapter_text(raw: &str) -> String {
	// Drop script / iframe / ins blocks entirely. The regex crate does not
	// support backreferences, so list the tags explicitly on both sides.
	let re_block = Regex::new(
		r"(?is)<(?:script|ins|iframe)[^>]*>.*?</(?:script|ins|iframe)>|<(?:script|ins|iframe)[^>]*/>",
	)
	.ok();
	if let Some(re) = re_block {
		let html = re.replace_all(raw, "");
		return convert_text(&html);
	}
	convert_text(raw)
}

fn convert_text(raw: &str) -> String {
	let mut html = raw.to_string();

	// Drop advertisement divs (identified by class/id keywords).
	let re_ad = Regex::new(
		r#"(?is)<div[^>]*(?:class|id)="[^"]*(?:\b|_)(?:ad|gg|ggw|slt|opera|promo)(?:\b|_)[^"]*"[^>]*>.*?</div>"#,
	)
	.ok();
	if let Some(re) = re_ad {
		html = re.replace_all(&html, "").into();
	}

	// Keep inline images as Markdown; then strip all other tags.
	let re_img = Regex::new(r#"(?i)<img[^>]*src="([^"]+)"[^>]*>"#).ok();
	if let Some(re) = re_img {
		html = re
			.replace_all(&html, |caps: &regex::Captures| format!("![]({})", &caps[1]))
			.into();
	}

	// Block-level ends become paragraph separators.
	let re_br = Regex::new(r"(?i)<br\s*/?>|</p>|</div>|</li>|</h\d>").ok();
	if let Some(re) = re_br {
		html = re.replace_all(&html, "\n\n").into();
	}

	let re_tag = Regex::new(r"(?s)<[^>]+>").ok();
	if let Some(re) = re_tag {
		html = re.replace_all(&html, "").into();
	}

	let mut text = html_unescape(&html);
	// Collapse 3+ newlines to a single paragraph break.
	let re_nl = Regex::new(r"\n{3,}").ok();
	if let Some(re) = re_nl {
		text = re.replace_all(&text, "\n\n").into();
	}
	text = text.replace("(本章完)", "").replace("（本章完）", "");
	text.trim().to_string()
}

fn html_unescape(html: &str) -> String {
	html.replace("&nbsp;", " ")
		.replace("&amp;", "&")
		.replace("&lt;", "<")
		.replace("&gt;", ">")
		.replace("&quot;", "\"")
		.replace("&#39;", "'")
		.replace("&ldquo;", "“")
		.replace("&rdquo;", "”")
		.replace("&hellip;", "…")
}
