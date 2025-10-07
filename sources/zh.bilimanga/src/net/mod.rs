use aidoku::{
	FilterValue, Result,
	alloc::{String, string::ToString as _},
	helpers::uri::encode_uri,
	imports::net::Request,
};
use core::fmt::{Display, Formatter, Result as FmtResult};

#[derive(Clone)]
pub enum Url {
	Filter {
		order: String,
		tagid: String,
		isfull: String,
		anime: String,
		rgroupid: String,
		sortid: String,
		update: String,
		quality: String,
		page: i32,
	},
	Search {
		query: String,
		page: i32,
	},
	Manga { id: String },
	ChapterList { id: String },
	Chapter { manga_id: String, chapter_id: String },
}


impl Url {
	pub fn request(&self) -> Result<Request> {
		let url = self.to_string();
		Ok(Request::get(url)?.header(
			"User-Agent",
			"Mozilla/5.0 (iPhone; CPU iPhone OS 16_6 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1",
		).header("Origin", "https://www.bilimanga.net").header("Accept-Language", "zh-CN,zh;q=0.9").header("Cookie", "night=0"))
	}

	pub fn from_query_or_filters(
		query: Option<&str>,
		page: i32,
		filters: &[FilterValue],
	) -> Result<Self> {
		if let Some(q) = query {
			return Ok(Self::Search {
				query: encode_uri(q.to_string()),
				page,
			});
		}

		let mut tagid = "0".to_string();
		let mut sortid = "0".to_string();
		let mut rgroupid = "0".to_string();
		let mut order = "lastupdate".to_string();
		let mut anime = "0".to_string();
		let mut quality = "0".to_string();
		let mut isfull = "0".to_string();
		let mut update = "0".to_string();

		for filter in filters {
			match filter {
				FilterValue::Text { value, .. } => {
					// Assuming title filter
					return Ok(Self::Search {
						query: encode_uri(value.clone()),
						page,
					});
				}
				FilterValue::Select { id, value } => match id.as_str() {
					"作品主题" => tagid = value.clone(),
					"作品分类" => sortid = value.clone(),
					"文库地区" => rgroupid = value.clone(),
					"是否动画" => anime = value.clone(),
					"是否轻改" => quality = value.clone(),
					"连载状态" => isfull = value.clone(),
					"更新时间" => update = value.clone(),
					_ => {}
				},
				FilterValue::Sort { index, .. } => {
					let orders = ["weekvisit", "monthvisit", "weekvote", "monthvote", "weekflower", "monthflower", "words", "goodnum", "lastupdate", "postdate"];
					if let Some(o) = orders.get(*index as usize) {
						order = o.to_string();
					}
				}
				_ => {}
			}
		}

		Ok(Self::Filter {
			order,
			tagid,
			isfull,
			anime,
			rgroupid,
			sortid,
			update,
			quality,
			page,
		})
	}

	pub fn manga(id: String) -> Self {
		Self::Manga { id }
	}

	pub fn chapter_list(id: String) -> Self {
		Self::ChapterList { id }
	}

	pub fn chapter(manga_id: String, chapter_id: String) -> Self {
		Self::Chapter { manga_id, chapter_id }
	}
}

impl Display for Url {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		let base = "https://www.bilimanga.net";
		match self {
			Url::Filter { order, tagid, isfull, anime, rgroupid, sortid, update, quality, page } => {
				write!(f, "{}/filter/{}_{}_{}_{}_{}_{}_{}_{}_{}_0.html", base, order, tagid, isfull, anime, rgroupid, sortid, update, quality, page)
			}
			Url::Search { query, page } => {
				write!(f, "{}/search/{}_{}.html", base, query, page)
			}
			Url::Manga { id } => {
				write!(f, "{}/detail/{}.html", base, id)
			}
			Url::ChapterList { id } => {
				write!(f, "{}/read/{}/catalog", base, id)
			}
			Url::Chapter { manga_id, chapter_id } => {
				write!(f, "{}/read/{}/{}.html", base, manga_id, chapter_id)
			}
		}
	}
}