use crate::{BASE_URL, USER_AGENT};
use aidoku::{
	alloc::{string::ToString as _, String},
	imports::net::Request,
	Result,
};
use core::fmt::{Display, Formatter, Result as FmtResult};

#[derive(Clone)]
pub enum Url {
	Manga { id: String },
}

impl Url {
	pub fn request(&self) -> Result<Request> {
		let url = self.to_string();
		let request = Request::get(url)?
			.header("User-Agent", USER_AGENT)
			.header("Accept-Language", "zh-TW");
		Ok(request)
	}

	pub fn manga(id: String) -> Self {
		Self::Manga { id }
	}
}

impl Display for Url {
	fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
		match self {
			Url::Manga { id } => {
				write!(f, "{}/{}", BASE_URL, id)
			}
		}
	}
}