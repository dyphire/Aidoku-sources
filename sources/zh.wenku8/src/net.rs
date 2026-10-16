//! Endpoint definitions and request building.
//!
//! Every page the source fetches is one `Url` variant; `Display` renders
//! the final address. The desktop site serves GBK HTML, the WAP site
//! serves UTF-8; decoding happens in html.rs.

use crate::gbk;
use crate::{BASE_URL, WAP_URL};
use aidoku::{
	Result,
	alloc::{String, Vec, string::ToString},
	imports::net::{HttpMethod, Request},
	prelude::*,
};
use core::fmt::{Display, Formatter, Result as FmtResult};

#[derive(Clone)]
pub enum Url {
	/// A `toplist.php` ranking, e.g. `sort=lastupdate`.
	Toplist { sort: &'static str, page: i32 },
	/// The `articlelist.php` catalog, `fullflag=1` for completed works.
	Catalog { fullflag: &'static str, page: i32 },
	Search {
		search_type: &'static str,
		query: String,
		page: i32,
	},
	/// The novel detail page (`articleinfo.php`).
	Novel { id: String },
	/// The volume/chapter tree (`reader.php`).
	Reader { id: String },
	/// A chapter body (`/novel/{bucket}/{aid}/{cid}.htm`).
	Chapter { aid: String, cid: String },
	/// The WAP home page, anonymously accessible and UTF-8 encoded.
	WapHome,
}

impl Url {
	pub fn request(&self) -> Result<Request> {
		let request = Request::get(self.to_string())?
			.header("Referer", &format!("{BASE_URL}/login.php"))
			.header(
				"Accept",
				"text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
			);
		Ok(request)
	}

	pub fn novel(id: &str) -> Self {
		Self::Novel { id: id.to_string() }
	}

	pub fn reader(id: &str) -> Self {
		Self::Reader { id: id.to_string() }
	}

	pub fn chapter(aid: &str, cid: &str) -> Self {
		Self::Chapter {
			aid: aid.to_string(),
			cid: cid.to_string(),
		}
	}

	/// The WAP search endpoint is not used; desktop search is GBK-encoded
	/// in the query string, so the query is stored raw and escaped here.
	pub fn search(search_type: &'static str, query: &str, page: i32) -> Self {
		Self::Search {
			search_type,
			query: query.to_string(),
			page,
		}
	}
}

impl Display for Url {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		match self {
			Url::Toplist { sort, page } => write!(
				f,
				"{BASE_URL}/modules/article/toplist.php?sort={sort}&page={page}&charset=gbk"
			),
			Url::Catalog { fullflag, page } => write!(
				f,
				"{BASE_URL}/modules/article/articlelist.php?fullflag={fullflag}&page={page}&charset=gbk"
			),
			Url::Search {
				search_type,
				query,
				page,
			} => write!(
				f,
				"{BASE_URL}/modules/article/search.php?searchtype={search_type}&searchkey={}&page={page}&charset=gbk",
				gbk::url_encode(query)
			),
			Url::Novel { id } => {
				write!(
					f,
					"{BASE_URL}/modules/article/articleinfo.php?id={id}&charset=gbk"
				)
			}
			Url::Reader { id } => {
				write!(
					f,
					"{BASE_URL}/modules/article/reader.php?aid={id}&charset=gbk"
				)
			}
			Url::Chapter { aid, cid } => {
				let bucket: u32 = aid.parse().unwrap_or(0) / 1000;
				write!(f, "{BASE_URL}/novel/{bucket}/{aid}/{cid}.htm")
			}
			Url::WapHome => write!(f, "{WAP_URL}/index.php"),
		}
	}
}

pub fn novel_url(aid: &str) -> String {
	format!("{BASE_URL}/book/{aid}.htm")
}

/// Cover images follow a fixed `img.wenku8.com` bucket layout by novel id.
pub fn cover_url(aid: &str) -> String {
	let bucket: u32 = aid.parse().unwrap_or(0) / 1000;
	format!("https://img.wenku8.com/image/{bucket}/{aid}/{aid}s.jpg")
}

/// Log in with a username/password pair, mirroring the reference client's
/// form submission. The login form's captcha is not enforced for ordinary
/// logins (verified against the live site: a credential check error is
/// returned instead of a captcha error). The session cookie is stored by
/// the host app and attached to subsequent requests automatically.
pub fn login(username: &str, password: &str) -> Result<bool> {
	// Warm-up visits seed the session cookies, as the reference client does.
	let _ = Request::get(format!("{BASE_URL}/"))?.send();
	let _ = Request::get(format!("{BASE_URL}/login.php"))?.send();

	let params = [
		("username", username),
		("password", password),
		("checkcode", ""),
		("usecookie", "315360000"),
		("action", "login"),
	];
	// The login page is GBK; a browser would submit the form GBK-encoded.
	let body = params
		.iter()
		.map(|(key, value)| format!("{key}={}", gbk::url_encode(value)))
		.collect::<Vec<_>>()
		.join("&");

	let mut request = Request::new(format!("{BASE_URL}/login.php"), HttpMethod::Post)?;
	request.set_header("Referer", &format!("{BASE_URL}/login.php"));
	request.set_header("Content-Type", "application/x-www-form-urlencoded");
	request.set_body(body.as_bytes());

	let response = request.send()?;
	let data = response.get_data()?;
	let text = gbk::decode(&data);
	if text.contains("Just a moment") || text.contains("__cf_chl_") {
		bail!("Cloudflare 验证拦截了登录请求，请稍后重试");
	}
	Ok(text.contains("登录成功"))
}
