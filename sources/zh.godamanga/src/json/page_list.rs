use crate::{API_URL, BASE_URL, IMG_URL};
use aidoku::{
	alloc::{string::ToString as _, String, Vec},
	imports::net::HttpMethod,
	prelude::format,
	AidokuError, Page, Result,
};

pub struct PageList;

impl PageList {
	pub fn get_pages(manga_id: String, chapter_id: String) -> Result<Vec<Page>> {
		let ids = manga_id.split("/").collect::<Vec<&str>>();
		let url = format!(
			"{}/api/chapter/getinfo?m={}&c={}",
			API_URL,
			ids[1],
			chapter_id.clone()
		);
		let json_text = aidoku::imports::net::Request::new(url.clone(), HttpMethod::Get)?
			.header("Origin", BASE_URL)
			.header("Referer", BASE_URL)
			.string()?;
		let json: serde_json::Value =
			serde_json::from_str(&json_text).map_err(|e| AidokuError::message(e.to_string()))?;
		let data = json
			.as_object()
			.ok_or_else(|| AidokuError::message("Expected JSON object"))?;
		let data = data
			.get("data")
			.and_then(|v| v.as_object())
			.ok_or_else(|| AidokuError::message("Expected data object"))?;
		let info = data
			.get("info")
			.and_then(|v| v.as_object())
			.ok_or_else(|| AidokuError::message("Expected info object"))?;
		let images = info
			.get("images")
			.and_then(|v| v.as_object())
			.ok_or_else(|| AidokuError::message("Expected images object"))?;
		let list = images
			.get("images")
			.and_then(|v| v.as_array())
			.ok_or_else(|| AidokuError::message("Expected images array"))?;
		let mut pages: Vec<Page> = Vec::new();

		for (index, item) in list.into_iter().enumerate() {
			let item = match item.as_object() {
				Some(item) => item,
				None => continue,
			};
			let _index = index as i32;
			let url = match item.get("url").and_then(|v| v.as_str()) {
				Some(url) => format!("{}{}", IMG_URL, url),
				None => continue,
			};
			pages.push(Page {
				content: aidoku::PageContent::Url(url, None),
				..Default::default()
			});
		}

		Ok(pages)
	}
}
