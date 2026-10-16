//! Parsers for wenku8 pages, as extension traits on the Aidoku HTML types.
//!
//! The desktop site serves GBK and answers anonymous listing requests with
//! a fake closure notice; both are surfaced as errors here. The WAP site
//! serves UTF-8 and is anonymously accessible.

use crate::net::Url;
use crate::net::novel_url;
use crate::{BASE_URL, gbk};
use aidoku::{
	Chapter, Manga, MangaPageResult, MangaStatus, Result,
	alloc::{String, Vec, string::ToString, vec},
	imports::html::{Document, Element, Html},
	imports::net::Response,
	prelude::*,
};

/// Desktop responses: decode GBK, detect the block pages, parse HTML.
pub fn document_gb(response: Response) -> Result<Document> {
	if response.status_code() != 200 {
		// 403/503 here is typically Cloudflare; the app intercepts those
		// statuses and runs its WebView challenge before we ever see them.
		bail!(
			"请求失败：HTTP {}（可能被 Cloudflare 拦截）",
			response.status_code()
		);
	}
	let text = gbk::decode(&response.get_data()?);
	if text.contains("__cf_chl_") || text.contains("Just a moment") {
		// Cloudflare sometimes answers the challenge page with a 200 status,
		// which bypasses the app-level handler; report it clearly.
		bail!("站点启用了 Cloudflare 验证：请在应用内打开 wenku8 网页完成验证后重试");
	}
	if text.contains("本站正式关闭") {
		// List endpoints answer anonymous requests with a fake closure
		// notice; only a logged-in session unlocks them.
		bail!("列表页需要登录：请先在图源设置中登录轻小说文库账号");
	}
	Html::parse(text).map_err(|_| error!("Failed to parse HTML"))
}

/// WAP responses: UTF-8, anonymous.
pub fn text_utf8(response: Response) -> Result<String> {
	if response.status_code() != 200 {
		bail!("请求失败：HTTP {}", response.status_code());
	}
	let text = response.get_string()?;
	if text.contains("__cf_chl_") || text.contains("Just a moment") {
		bail!("站点启用了 Cloudflare 验证，请稍后重试");
	}
	Ok(text)
}

/// Fetch a desktop (GBK) page and parse it.
pub fn fetch_gb(url: &Url) -> Result<Document> {
	document_gb(url.request()?.send()?)
}

/// Fetch a WAP (UTF-8) page as text.
pub fn fetch_utf8_text(url: &Url) -> Result<String> {
	text_utf8(url.request()?.send()?)
}

trait TrySelect {
	fn try_select_first<S: AsRef<str>>(&self, css_query: S) -> Result<Element>;
}

impl TrySelect for Document {
	fn try_select_first<S: AsRef<str>>(&self, css_query: S) -> Result<Element> {
		self.select_first(&css_query)
			.ok_or_else(|| error!("No element found for selector: `{}`", css_query.as_ref()))
	}
}

pub trait ListPage {
	/// A ranking/catalog/search result page (`table.grid` entries plus the
	/// `em#pagestats` pagination).
	fn manga_page_result(&self) -> MangaPageResult;
}

impl ListPage for Document {
	fn manga_page_result(&self) -> MangaPageResult {
		let mut entries = vec![];
		if let Some(blocks) = self.select("table.grid tr td > div") {
			for block in blocks {
				let Some(anchor) = block.select_first("a") else {
					continue;
				};
				// Only blocks whose link wraps a cover image are novel entries.
				let Some(img) = anchor.select_first("img") else {
					continue;
				};
				let Some(href) = anchor.attr("href") else {
					continue;
				};
				let Some(aid) = parse_aid(&href) else {
					continue;
				};
				let title = anchor.attr("title").unwrap_or_default();
				let cover = img.attr("src").map(|src| absolute_url(&src));
				entries.push(Manga {
					key: aid,
					title,
					cover,
					url: Some(absolute_url(&href)),
					..Default::default()
				});
			}
		}
		let has_next_page = match self.select_first("em#pagestats").and_then(|em| em.text()) {
			Some(stats) => {
				let mut parts = stats.split('/');
				let current = parts.next().and_then(|s| s.trim().parse::<i32>().ok());
				let max = parts.next().and_then(|s| s.trim().parse::<i32>().ok());
				match (current, max) {
					(Some(current), Some(max)) => current < max,
					_ => false,
				}
			}
			None => false,
		};
		MangaPageResult {
			entries,
			has_next_page,
		}
	}
}

pub trait NovelPage {
	/// Fill title/author/status/tags/description from `articleinfo.php`.
	fn update_details(&self, manga: &mut Manga) -> Result<()>;
}

impl NovelPage for Document {
	fn update_details(&self, manga: &mut Manga) -> Result<()> {
		let content = self.try_select_first("#content")?;

		// The detail page labels its metadata cells/spans explicitly
		// (`小说作者：` / `文章状态：` / `作品Tags：` / `内容简介：`), so
		// label matching is used instead of positional table indexing.
		manga.title = content
			.select_first("table span b")
			.and_then(|b| b.text())
			.map(|t| t.trim().to_string())
			.unwrap_or_default();

		for td in content
			.select("td")
			.map(|els| els.collect::<Vec<_>>())
			.unwrap_or_default()
		{
			if let Some(author) = cell_text_after_label(&td, "小说作者：")
				.or_else(|| cell_text_after_label(&td, "小说作者:"))
			{
				manga.authors = Some(vec![author]);
			}
			if let Some(value) = cell_text_after_label(&td, "文章状态：")
				.or_else(|| cell_text_after_label(&td, "文章状态:"))
			{
				manga.status = parse_status(&value);
			}
		}

		manga.cover = content
			.select_first("img")
			.and_then(|img| img.attr("src"))
			.map(|src| absolute_url(&src));

		let mut description = None;
		let spans: Vec<Element> = content
			.select("span")
			.map(|els| els.collect())
			.unwrap_or_default();
		for (index, span) in spans.iter().enumerate() {
			let text = span.text().unwrap_or_default();
			if let Some(value) = text
				.trim()
				.strip_prefix("作品Tags：")
				.or_else(|| text.trim().strip_prefix("作品Tags:"))
			{
				let tags: Vec<String> = value
					.split_whitespace()
					.map(ToString::to_string)
					.filter(|s| !s.is_empty())
					.collect();
				if !tags.is_empty() {
					manga.tags = Some(tags);
				}
			}
			// The introduction follows the `内容简介：` label span, wrapped
			// in its own span after a <br>.
			if text.trim() == "内容简介：" || text.trim() == "内容简介:" {
				for next in &spans[index + 1..] {
					let body = next.text().unwrap_or_default();
					if body.trim().is_empty() {
						continue;
					}
					let mut raw = String::new();
					crate::markdown::extract_text(next, &mut raw);
					description = Some(crate::markdown::text_to_markdown(&raw));
					break;
				}
			}
		}
		manga.description = description;

		manga.url = Some(novel_url(&manga.key));
		Ok(())
	}
}

pub trait ReaderPage {
	/// Flatten the volume/chapter tree (`table.css`) into a chapter list,
	/// newest first.
	fn chapters(&self) -> Result<Vec<Chapter>>;
}

impl ReaderPage for Document {
	fn chapters(&self) -> Result<Vec<Chapter>> {
		let table = self
			.select_first("table.css")
			.ok_or_else(|| error!("Reader page missing table.css"))?;
		let mut chapters = vec![];
		// Volume rows carry a database id in `vid` (e.g. 73728), not a
		// sequence, so volumes are numbered by visit order; the number only
		// serves as a tie-breaker when the user sorts by chapter number.
		let mut volume_number: f32 = 0.0;
		for row in table
			.select("tr")
			.map(|els| els.collect::<Vec<_>>())
			.unwrap_or_default()
		{
			if row.select_first("td.vcss").is_some() {
				volume_number += 1.0;
				continue;
			}
			let Some(links) = row.select("td.ccss > a") else {
				continue;
			};
			for anchor in links {
				let Some(href) = anchor.attr("href") else {
					continue;
				};
				let Some(cid) = query_value(&href, "cid") else {
					continue;
				};
				let aid = query_value(&href, "aid").unwrap_or_default();
				let title = anchor.text().unwrap_or_default().trim().to_string();
				chapters.push(Chapter {
					key: format!("{aid}:{cid}"),
					chapter_number: parse_chapter_number(&title),
					title: Some(title),
					volume_number: (volume_number > 0.0).then_some(volume_number),
					url: Some(absolute_url(&href)),
					language: Some("zh".to_string()),
					..Default::default()
				});
			}
		}
		// The site lists chapters oldest-first; Aidoku renders source order
		// as-is with the newest chapter on top, so reverse before returning.
		chapters.reverse();
		Ok(chapters)
	}
}

fn cell_text_after_label(td: &Element, label: &str) -> Option<String> {
	let text = td.text().unwrap_or_default();
	text.trim()
		.strip_prefix(label)
		.map(|rest| rest.trim().to_string())
		.filter(|rest| !rest.is_empty())
}

fn parse_status(value: &str) -> MangaStatus {
	match value {
		"已完成" | "完结" | "已完结" => MangaStatus::Completed,
		"连载中" | "连载" => MangaStatus::Ongoing,
		_ => MangaStatus::Unknown,
	}
}

fn absolute_url(href: &str) -> String {
	if href.starts_with("http") {
		href.into()
	} else if let Some(rest) = href.strip_prefix('/') {
		format!("{BASE_URL}/{rest}")
	} else {
		format!("{BASE_URL}/{href}")
	}
}

/// Extract the novel id (aid) from a detail-page link. Links come in two
/// shapes: `/book/2070.htm` and `/modules/article/articleinfo.php?id=2070`.
fn parse_aid(href: &str) -> Option<String> {
	if let Some(query) = href.split('?').nth(1) {
		for pair in query.split('&') {
			if let Some(value) = pair.strip_prefix("id=") {
				let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
				if !digits.is_empty() {
					return Some(digits);
				}
			}
		}
	}
	let segment = href.split('/').next_back()?;
	let id = segment.trim_end_matches(".htm");
	if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
		Some(id.to_string())
	} else {
		None
	}
}

fn query_value(href: &str, key: &str) -> Option<String> {
	let query = href.split('?').nth(1)?;
	for pair in query.split('&') {
		if let Some(value) = pair.strip_prefix(key).and_then(|s| s.strip_prefix('=')) {
			return Some(value.chars().take_while(|c| c.is_ascii_digit()).collect());
		}
	}
	None
}

/// Parse a chapter number out of titles like `第十二章 xxx`, `第12话 xxx`,
/// or `1.社团活动部`. Arabic numerals win; common Chinese numerals (up to
/// the thousands) are converted. Titles without a number (序章/后记)
/// return None: Aidoku's default source-order sort keeps them in place.
pub fn parse_chapter_number(title: &str) -> Option<f32> {
	let rest = title.trim_start_matches("第");
	let numerals = "零一二三四五六七八九十百千";
	let mut value = String::new();
	let mut chinese = String::new();
	let mut in_number = false;
	for ch in rest.chars() {
		if ch.is_ascii_digit() || (ch == '.' && in_number) {
			in_number = true;
			value.push(ch);
		} else if !in_number && numerals.contains(ch) {
			chinese.push(ch);
		} else if in_number || !chinese.is_empty() {
			break;
		}
	}
	if !value.is_empty() {
		return value.parse().ok();
	}
	if !chinese.is_empty() {
		return chinese_numeral(&chinese).map(|n| n as f32);
	}
	None
}

fn chinese_numeral(text: &str) -> Option<u32> {
	// Handles forms like 十二 (12), 二十 (20), 一百二十三 (123).
	let mut total = 0u32;
	let mut section = 0u32;
	let mut number = 0u32;
	for c in text.chars() {
		match c {
			'零' => {}
			'一' | '二' | '两' | '三' | '四' | '五' | '六' | '七' | '八' | '九' => {
				number = match c {
					'一' => 1,
					'二' | '两' => 2,
					'三' => 3,
					'四' => 4,
					'五' => 5,
					'六' => 6,
					'七' => 7,
					'八' => 8,
					_ => 9,
				};
			}
			'十' => {
				section += if number == 0 { 1 } else { number } * 10;
				number = 0;
			}
			'百' => {
				section = (if number == 0 { 1 } else { number } + section) * 100;
				number = 0;
			}
			'千' => {
				section = (if number == 0 { 1 } else { number } + section) * 1000;
				number = 0;
			}
			_ => return None,
		}
	}
	total += section + number;
	Some(total)
}
