import requests
from bs4 import BeautifulSoup
import re
import sys

# Constants
BASE_URL = "https://www.dm5.com"
USER_AGENT = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/139.0.0.0 Safari/537.36"
HEADERS = {
    "User-Agent": USER_AGENT,
    "Accept-Language": "zh-TW",
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    "Referer": BASE_URL,
    "DNT": "1"
}

def unpack(packed):
    """JavaScript Packer Unpacker"""
    if "eval(function(p,a,c,k,e," not in packed:
        return packed
    
    # Extract the packed data - more flexible pattern
    try:
        # Find the final function call: }('p_string', a_num, c_num, 'k_string'.split('|'), ...)
        # Use a simpler approach: find between }(' and '.split
        start_marker = "}('"
        split_marker = "'.split('|')"
        
        start_idx = packed.find(start_marker)
        if start_idx == -1:
            print("Warning: Could not find start marker }\('")
            return packed
        
        split_idx = packed.find(split_marker, start_idx)
        if split_idx == -1:
            print("Warning: Could not find split marker")
            return packed
        
        # Extract the section: 'p_string', a_num, c_num, 'k_string'
        section = packed[start_idx + len(start_marker):split_idx]
        
        # Split by ',' but be careful with strings
        # Find the end of first string (p)
        p_end = section.find("',")
        if p_end == -1:
            print("Warning: Could not find end of p string")
            return packed
        
        p = section[0:p_end]
        remaining = section[p_end + 2:].strip()  # Skip ', and whitespace
        
        # Now we have: a_num, c_num, 'k_string'
        # Find the start of k string
        k_start = remaining.rfind(",'")
        if k_start == -1:
            print("Warning: Could not find start of k string")
            return packed
        
        # Extract a and c (numbers between p and k)
        nums_str = remaining[:k_start].strip()
        nums = [int(x.strip()) for x in nums_str.split(',')]
        if len(nums) < 2:
            print(f"Warning: Could not extract a and c, got: {nums}")
            return packed
        
        a = nums[0]
        c = nums[1]
        
        # Extract k string
        k_str = remaining[k_start + 2:].strip()  # Skip ,'
        
        # Handle escape sequences in p
        p = p.replace("\\\\", "\\").replace("\\'", "'").replace("\\n", "\n").replace("\\r", "\r")
        
        keywords = k_str.split('|')
        
        print(f"[Unpacker] p length: {len(p)}, a: {a}, c: {c}, keywords: {len(keywords)}")
        
        # Unpack
        def to_base(num, radix):
            if num == 0:
                return "0"
            digits = "0123456789abcdefghijklmnopqrstuvwxyz"
            result = ""
            while num > 0:
                result = digits[num % radix] + result
                num //= radix
            return result
        
        result = p
        for i in range(c - 1, -1, -1):
            base_str = to_base(i, a)
            replacement = keywords[i] if i < len(keywords) and keywords[i] else base_str
            if replacement:
                # Use word boundary replacement
                pattern = r'\b' + re.escape(base_str) + r'\b'
                result = re.sub(pattern, replacement, result)
        
        print(f"[Unpacker] Unpacked successfully, result length: {len(result)}")
        return result
    except Exception as e:
        print(f"Error in unpacker: {e}")
        import traceback
        traceback.print_exc()
        return packed

def get_page_list(chapter_url):
    print(f"=" * 60)
    print(f"Requesting chapter: {chapter_url}")
    print(f"=" * 60)
    
    response = requests.get(chapter_url, headers=HEADERS)
    response.raise_for_status()
    html = response.text
    soup = BeautifulSoup(html, 'html.parser')

    # Check for direct images
    images = soup.select("div#barChapter > img.load-src")
    if images:
        print(f"✓ Found {len(images)} direct images")
        pages = []
        for idx, img in enumerate(images, 1):
            data_src = img.get('data-src', '')
            if data_src:
                print(f"  [{idx}] {data_src}")
                pages.append(data_src)
        return pages

    # Check for packed images
    print("No direct images found, checking for packed images...")
    script = soup.find('script', string=lambda t: t and 'DM5_MID' in t)
    
    if not script:
        # Check if it's a paid chapter
        pay_form = soup.select_one("div.view-pay-form p.subtitle")
        if pay_form:
            raise Exception(f"Paid/Locked chapter: {pay_form.get_text(strip=True)}")
        raise Exception("No script with DM5_MID found")
    
    script_text = script.string
    print(f"✓ Found script with DM5_MID (length: {len(script_text)} chars)")
    
    if 'DM5_VIEWSIGN_DT' not in script_text:
        print("✗ DM5_VIEWSIGN_DT not found in script")
        # Check if it's a paid chapter
        pay_form = soup.select_one("div.view-pay-form p.subtitle")
        if pay_form:
            raise Exception(f"Paid/Locked chapter: {pay_form.get_text(strip=True)}")
        raise Exception("Chapter not available (missing DM5_VIEWSIGN_DT)")

    # Extract variables
    print("\nExtracting variables...")
    try:
        cid = extract_var(script_text, 'DM5_CID')
        print(f"  ✓ CID: {cid}")
    except Exception as e:
        print(f"  ✗ CID: {e}")
        raise
    
    try:
        mid = extract_var(script_text, 'DM5_MID')
        print(f"  ✓ MID: {mid}")
    except Exception as e:
        print(f"  ✗ MID: {e}")
        raise
    
    try:
        dt = extract_var(script_text, 'DM5_VIEWSIGN_DT').strip('"')
        print(f"  ✓ DT: {dt}")
    except Exception as e:
        print(f"  ✗ DT: {e}")
        raise
    
    try:
        sign = extract_var(script_text, 'DM5_VIEWSIGN').strip('"')
        print(f"  ✓ SIGN: {sign}")
    except Exception as e:
        print(f"  ✗ SIGN: {e}")
        raise
    
    try:
        image_count = int(extract_var(script_text, 'DM5_IMAGE_COUNT'))
        print(f"  ✓ Image Count: {image_count}")
    except Exception as e:
        print(f"  ✗ Image Count: {e}")
        raise

    # Test first page
    print(f"\n{'=' * 60}")
    print("Testing first page URL decryption...")
    print(f"{'=' * 60}")
    
    base_url = chapter_url.rstrip('/')
    chapterfun_url = f"{base_url}/chapterfun.ashx?cid={cid}&page=1&key=&language=1&gtk=6&_cid={cid}&_mid={mid}&_dt={dt}&_sign={sign}"
    print(f"Request URL: {chapterfun_url}")
    
    try:
        response = requests.get(chapterfun_url, headers={
            "User-Agent": USER_AGENT,
            "Referer": base_url
        })
        response.raise_for_status()
        js_code = response.text
        print(f"✓ Response received (length: {len(js_code)} chars)")
        
        # Save raw response
        with open("chapterfun_raw.txt", "w", encoding="utf-8") as f:
            f.write(js_code)
        print("  Raw response saved to: chapterfun_raw.txt")
        
        # Unpack
        print("\nUnpacking JavaScript...")
        unpacked = unpack(js_code)
        
        # Save unpacked
        with open("chapterfun_unpacked.txt", "w", encoding="utf-8") as f:
            f.write(unpacked)
        print("  Unpacked code saved to: chapterfun_unpacked.txt")
        
        # Extract image URL
        print("\nExtracting image URL components...")
        
        pix_match = re.search(r'var pix="([^"]+)"', unpacked)
        if pix_match:
            pix = pix_match.group(1)
            print(f"  ✓ pix: {pix}")
        else:
            print(f"  ✗ pix: not found")
            print(f"  Searching for 'var pix' in unpacked code...")
            if 'var pix' in unpacked:
                idx = unpacked.find('var pix')
                print(f"  Found at position {idx}: {unpacked[idx:idx+100]}...")
            raise Exception("Failed to extract pix")
        
        # pvalue is an array: var pvalue=["/file1.jpg","/file2.jpg"]
        # Extract all values from the array
        pvalue_match = re.search(r'var pvalue=\[([^\]]+)\]', unpacked)
        if pvalue_match:
            pvalue_str = pvalue_match.group(1)
            # Parse the array elements
            pvalue_list = re.findall(r'"([^"]+)"', pvalue_str)
            print(f"  ✓ pvalue array: {pvalue_list}")
            print(f"  ✓ pvalue array length: {len(pvalue_list)}")
            # For page 1, use the first element
            pvalue = pvalue_list[0] if pvalue_list else ""
            print(f"  ✓ pvalue (for page 1): {pvalue}")
        else:
            print(f"  ✗ pvalue: not found")
            print(f"  Searching for 'var pvalue' in unpacked code...")
            if 'var pvalue' in unpacked:
                idx = unpacked.find('var pvalue')
                print(f"  Found at position {idx}: {unpacked[idx:idx+100]}...")
            raise Exception("Failed to extract pvalue")
        
        query_match = re.search(r'pix\+pvalue\[i\]\+\'([^\']+)\'', unpacked)
        if query_match:
            query = query_match.group(1)
            print(f"  ✓ query: {query}")
        else:
            print(f"  ✗ query: not found")
            print(f"  Searching for 'pix+pvalue' in unpacked code...")
            if 'pix+pvalue' in unpacked:
                idx = unpacked.find('pix+pvalue')
                print(f"  Found at position {idx}: {unpacked[idx:idx+100]}...")
            raise Exception("Failed to extract query")
        
        image_url = f"{pix}{pvalue}{query}"
        print(f"\n✓ Final image URL (page 1): {image_url}")
        
        # Test page 2 to see what pvalue it returns
        print(f"\n{'=' * 60}")
        print("Testing page 2 URL decryption...")
        print(f"{'=' * 60}")
        chapterfun_url_page2 = f"{base_url}/chapterfun.ashx?cid={cid}&page=2&key=&language=1&gtk=6&_cid={cid}&_mid={mid}&_dt={dt}&_sign={sign}"
        print(f"Request URL: {chapterfun_url_page2}")
        
        response2 = requests.get(chapterfun_url_page2, headers={
            "User-Agent": USER_AGENT,
            "Referer": base_url
        })
        response2.raise_for_status()
        js_code2 = response2.text
        unpacked2 = unpack(js_code2)
        
        pvalue_match2 = re.search(r'var pvalue=\[([^\]]+)\]', unpacked2)
        if pvalue_match2:
            pvalue_str2 = pvalue_match2.group(1)
            pvalue_list2 = re.findall(r'"([^"]+)"', pvalue_str2)
            print(f"  ✓ Page 2 pvalue array: {pvalue_list2}")
            print(f"  ✓ Page 2 pvalue array length: {len(pvalue_list2)}")
            print(f"  → Observation: Page 2 request returns images for page {2} (index 0)")
        
    except Exception as e:
        print(f"✗ Error testing first page: {e}")
        import traceback
        traceback.print_exc()
        raise
    
    # Generate all page URLs (just the chapterfun URLs for now)
    print(f"\n{'=' * 60}")
    print(f"Generating URLs for all {image_count} pages...")
    print(f"{'=' * 60}")
    
    pages = []
    for i in range(1, image_count + 1):
        page_url = f"{base_url}/chapterfun.ashx?cid={cid}&page={i}&key=&language=1&gtk=6&_cid={cid}&_mid={mid}&_dt={dt}&_sign={sign}"
        pages.append(page_url)
        if i <= 3 or i == image_count:  # Show first 3 and last
            print(f"  [{i}] {page_url}")
        elif i == 4:
            print(f"  ...")
    
    return pages

def extract_var(script, var_name):
    start = f"var {var_name}="
    idx = script.find(start)
    if idx == -1:
        raise Exception(f"{var_name} not found in script")
    start_idx = idx + len(start)
    
    # Check if value is quoted
    if start_idx < len(script) and script[start_idx] == '"':
        end_char = '"'
        start_idx += 1
    else:
        end_char = ';'
    
    end_idx = script.find(end_char, start_idx)
    if end_idx == -1:
        raise Exception(f"Could not find end of {var_name} value")
    
    value = script[start_idx:end_idx].strip()
    return value

if __name__ == "__main__":
    # Default test chapter, can be overridden by command line argument
    chapter_url = "https://www.dm5.com/m1203034/" if len(sys.argv) < 2 else sys.argv[1]
    
    try:
        pages = get_page_list(chapter_url)
        print(f"\n{'=' * 60}")
        print(f"✓ SUCCESS: Found {len(pages)} pages")
        print(f"{'=' * 60}")
        
        # Test downloading the first page image
        print(f"\n{'=' * 60}")
        print("Testing image download...")
        print(f"{'=' * 60}")
        
        # Get first page chapterfun URL
        first_page_url = pages[0]
        print(f"Fetching first page data from: {first_page_url}")
        
        response = requests.get(first_page_url, headers={
            "User-Agent": USER_AGENT,
            "Referer": chapter_url.rstrip('/')
        })
        response.raise_for_status()
        js_code = response.text
        
        # Unpack
        unpacked = unpack(js_code)
        
        # Extract image URL components
        pix_match = re.search(r'var pix="([^"]+)"', unpacked)
        pix = pix_match.group(1) if pix_match else None
        
        pvalue_match = re.search(r'var pvalue=\[([^\]]+)\]', unpacked)
        if pvalue_match:
            pvalue_str = pvalue_match.group(1)
            pvalue_list = re.findall(r'"([^"]+)"', pvalue_str)
            pvalue = pvalue_list[0] if pvalue_list else None
        else:
            pvalue = None
        
        query_match = re.search(r'pix\+pvalue\[i\]\+\'([^\']+)\'', unpacked)
        query = query_match.group(1) if query_match else None
        
        if pix and pvalue and query:
            image_url = f"{pix}{pvalue}{query}"
            print(f"✓ Image URL: {image_url}")
            
            # Download the image
            print("Downloading image...")
            img_response = requests.get(image_url, headers={
                "User-Agent": USER_AGENT,
                "Referer": chapter_url.rstrip('/')
            })
            img_response.raise_for_status()
            
            # Save to file
            filename = "test_image.jpg"
            with open(filename, "wb") as f:
                f.write(img_response.content)
            
            print(f"✓ Image downloaded successfully!")
            print(f"  File: {filename}")
            print(f"  Size: {len(img_response.content)} bytes")
            print(f"  Content-Type: {img_response.headers.get('Content-Type', 'unknown')}")
        else:
            print(f"✗ Failed to extract image URL components")
            print(f"  pix: {pix}")
            print(f"  pvalue: {pvalue}")
            print(f"  query: {query}")
        
    except Exception as e:
        print(f"\n{'=' * 60}")
        print(f"✗ FAILED: {e}")
        print(f"{'=' * 60}")
        import traceback
        traceback.print_exc()
        sys.exit(1)