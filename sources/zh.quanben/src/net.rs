//! URL construction, request throttling/retries and the search-keyword
//! obfuscation algorithm for quanben5.

use crate::{BASE_URL, USER_AGENT};
use aidoku::{
	Result,
	alloc::{String, Vec, string::ToString as _},
	helpers::uri::encode_uri,
	imports::net::Request,
};
use core::fmt::{Display, Formatter, Result as FmtResult};

/// The 62-char substitution alphabet used by the site's search endpoint.
/// The keywords are percent-encoded, then every char is shifted +3 within
/// this alphabet (unchanged chars pass through) and each result char is
/// padded with two random alphabet chars (`b` parameter). Randomness here
/// comes from a small LCG since the Aidoku runtime has no RNG.
const ALPHABET: &[u8] = b"PXhw7UT1B0a9kQDKZsjIASmOezxYG4CHo5Jyfg2b8FLpEvRr3WtVnlqMidu6cN";

/// Default browse category when the user hasn't picked a filter (玄幻).
pub const DEFAULT_CATEGORY: &str = "1";

pub enum Url {
	/// Encrypted JSONP search: `/?c=book&a=search.json&callback=search&keywords=…&b=…`.
	Search { query: String, rng: u32 },
	/// `/{category}.html` for page 1, `/{category}_{page}.html` afterwards.
	List { category: String, page: i32 },
	/// `/n/{id}/` — book details page.
	Manga { id: String },
	/// `/n/{id}/xiaoshuo.html` — the full chapter list.
	Toc { id: String },
	/// `/n/{id}/{chapter}.html` — a single chapter body page (one page per
	/// chapter on this site; the "下一页" link points to the next chapter).
	Chapter { id: String, chapter: String },
}

impl Url {
	pub fn request(&self) -> Result<Request> {
		Ok(Request::get(self.to_string())?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-CN,zh;q=0.9")
			.header("Referer", BASE_URL))
	}

	/// Pick a category from the filters for browsing, defaulting to 玄幻.
	pub fn from_filters(filters: Vec<aidoku::FilterValue>, page: i32) -> Self {
		let mut category = String::from(DEFAULT_CATEGORY);
		for filter in filters {
			if let aidoku::FilterValue::Select { id, value } = filter
				&& id == "分類"
			{
				category = value;
			}
		}
		Self::List { category, page }
	}

	pub fn manga(id: &str) -> Self {
		Self::Manga { id: id.to_string() }
	}

	pub fn toc(id: &str) -> Self {
		Self::Toc { id: id.to_string() }
	}

	pub fn chapter(id: &str, chapter: &str) -> Self {
		Self::Chapter {
			id: id.to_string(),
			chapter: chapter.to_string(),
		}
	}
}

impl Display for Url {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		match self {
			Self::Search { query, rng } => {
				let kw = encode_uri(query);
				let mut state = *rng;
				let mut e = String::new();
				for &byte in kw.as_bytes() {
					let shifted = match ALPHABET.iter().position(|&c| c == byte) {
						Some(i) => ALPHABET[(i + 3) % ALPHABET.len()],
						None => byte,
					};
					state = state.wrapping_mul(1664525).wrapping_add(1013904223);
					let n1 = (state as usize) % ALPHABET.len();
					state = state.wrapping_mul(1664525).wrapping_add(1013904223);
					let n2 = (state as usize) % ALPHABET.len();
					e.push(ALPHABET[n1] as char);
					e.push(shifted as char);
					e.push(ALPHABET[n2] as char);
				}
				write!(
					f,
					"{BASE_URL}/?c=book&a=search.json&callback=search&keywords={kw}&b={}",
					encode_uri(&e)
				)
			}
			Self::List { category, page } => {
				if *page <= 1 {
					write!(f, "{BASE_URL}/category/{category}.html")
				} else {
					write!(f, "{BASE_URL}/category/{category}_{page}.html")
				}
			}
			Self::Manga { id } => write!(f, "{BASE_URL}/n/{id}/"),
			Self::Toc { id } => write!(f, "{BASE_URL}/n/{id}/xiaoshuo.html"),
			Self::Chapter { id, chapter } => write!(f, "{BASE_URL}/n/{id}/{chapter}.html"),
		}
	}
}

/// Whether a listing page has a "下一页" link (pagination).
pub fn has_next_page(document: &aidoku::imports::html::Document) -> bool {
	document.select("a").is_some_and(|mut links| {
		links.any(|a| {
			a.text()
				.is_some_and(|t| t.trim() == "下一页" || t.contains("下一页"))
		})
	})
}
