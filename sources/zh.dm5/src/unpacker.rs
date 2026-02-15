use aidoku::{alloc::{String, Vec, string::ToString}, prelude::*, Result};

/// JavaScript Packer Unpacker
/// Port from Python debug implementation
pub fn unpack(packed: &str) -> Result<String> {
	// Check if the string is packed
	if !packed.contains("eval(function(p,a,c,k,e,") {
		return Ok(packed.to_string());
	}

	// Extract the packed data using simpler string matching
	// Format: }('p_string', a_num, c_num, 'k_string'.split('|'), ...)
	
	let start_marker = "}('";
	let split_marker = "'.split('|')";
	
	let start_idx = match packed.find(start_marker) {
		Some(idx) => idx + start_marker.len(),
		None => return Ok(packed.to_string()),
	};
	
	let split_idx = match packed[start_idx..].find(split_marker) {
		Some(idx) => start_idx + idx,
		None => return Ok(packed.to_string()),
	};
	
	// Extract the section: 'p_string', a_num, c_num, 'k_string'
	let section = &packed[start_idx..split_idx];
	
	// Find the end of first string (p)
	let p_end = match section.find("',") {
		Some(idx) => idx,
		None => return Ok(packed.to_string()),
	};
	let p = &section[0..p_end];
	
	// Parse a and c
	let rest = &section[p_end + 2..];
	let parts: Vec<&str> = rest.split(',').collect();
	if parts.len() < 3 {
		return Ok(packed.to_string());
	}
	
	let a: usize = match parts[0].trim().parse() {
		Ok(val) => val,
		Err(_) => return Ok(packed.to_string()),
	};
	
	let c: usize = match parts[1].trim().parse() {
		Ok(val) => val,
		Err(_) => return Ok(packed.to_string()),
	};
	
	// Extract k_string (between quotes in parts[2])
	let k_part = parts[2].trim();
	let k_start = match k_part.find('\'') {
		Some(idx) => idx + 1,
		None => return Ok(packed.to_string()),
	};
	let k_end = match k_part[k_start..].find('\'') {
		Some(idx) => k_start + idx,
		None => return Ok(packed.to_string()),
	};
	let k = &k_part[k_start..k_end];
	
	// Split keywords
	let keywords: Vec<&str> = k.split('|').collect();
	
	// Unpack using the algorithm
	let mut result = String::from(p);
	
	for i in (0..c).rev() {
		let replacement = if i < keywords.len() && !keywords[i].is_empty() {
			keywords[i]
		} else {
			// Convert i to base-a representation
			&to_base(i, a)
		};
		
		let pattern = to_base(i, a);
		result = replace_word(&result, &pattern, replacement);
	}
	
	Ok(result)
}

/// Convert number to base-a representation (like base 36, but up to 62)
fn to_base(mut num: usize, radix: usize) -> String {
	if num == 0 {
		return String::from("0");
	}
	
	let chars = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
	let mut result = Vec::new();
	
	while num > 0 {
		result.push(chars[num % radix]);
		num /= radix;
	}
	
	result.reverse();
	String::from_utf8(result).unwrap_or_else(|_| String::from("0"))
}

/// Replace word with boundary matching
fn replace_word(text: &str, pattern: &str, replacement: &str) -> String {
	let mut result = String::new();
	let mut last_end = 0;
	
	for (idx, _) in text.match_indices(pattern) {
		// Check if this match is a whole word
		let before_is_boundary = idx == 0 || !text.as_bytes()[idx - 1].is_ascii_alphanumeric();
		let after_idx = idx + pattern.len();
		let after_is_boundary = after_idx >= text.len() || !text.as_bytes()[after_idx].is_ascii_alphanumeric();
		
		if before_is_boundary && after_is_boundary {
			// This is a whole word match
			result.push_str(&text[last_end..idx]);
			result.push_str(replacement);
			last_end = after_idx;
		}
	}
	
	result.push_str(&text[last_end..]);
	result
}

#[cfg(test)]
mod tests {
	use super::*;
	
	#[test]
	fn test_to_base() {
		assert_eq!(to_base(0, 36), "0");
		assert_eq!(to_base(10, 36), "a");
		assert_eq!(to_base(35, 36), "z");
		assert_eq!(to_base(36, 36), "10");
	}
	
	#[test]
	fn test_replace_word() {
		let text = "var i=0; i<10; i++";
		let result = replace_word(text, "i", "index");
		assert_eq!(result, "var index=0; index<10; index++");
		
		// Should not replace 'i' in 'if'
		let text2 = "if i<10";
		let result2 = replace_word(text2, "i", "index");
		assert_eq!(result2, "if index<10");
	}
}
