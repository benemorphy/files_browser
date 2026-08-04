// pdf2html.rs — Pure Rust PDF→HTML converter
// Architecture: lopdf parser → content stream interpreter → HTML output
// Status: MVP — handles common academic/technical PDFs with basic text extraction

use lopdf::{
    Document, Object, ObjectId, Stream,
    content::{Content, Operation},
};
use std::collections::HashMap;

// ===== Constants =====
const DPI: f64 = 96.0;           // CSS output DPI
const PT_TO_PX: f64 = DPI / 72.0; // 1pt = 1.333px at 96dpi

// ===== Graphics State =====
#[derive(Clone)]
struct TextState {
    font_name: String,        // font resource name (e.g. /F1)
    font_size: f64,
    char_spacing: f64,
    word_spacing: f64,
    h_scale: f64,             // horizontal scaling (percent)
    leading: f64,
    rise: f64,
}

impl Default for TextState {
    fn default() -> Self {
        Self {
            font_name: String::new(),
            font_size: 12.0,
            char_spacing: 0.0,
            word_spacing: 0.0,
            h_scale: 100.0,
            leading: 14.5,
            rise: 0.0,
        }
    }
}

#[derive(Clone)]
struct GraphicsState {
    ctm: [f64; 6],            // current transformation matrix [a,b,c,d,e,f]
    text_matrix: [f64; 6],    // text matrix
    text_line_matrix: [f64; 6], // text line matrix
    in_text: bool,            // inside BT..ET
    text: TextState,
    fill_r: f64,              // fill color (RGB 0-1)
    fill_g: f64,
    fill_b: f64,
    fill_set: bool,
    path_start: Option<(f64, f64)>,
    segments: Vec<PathSegment>,
}

impl Default for GraphicsState {
    fn default() -> Self {
        Self {
            ctm: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            text_matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            text_line_matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            in_text: false,
            text: TextState::default(),
            fill_r: 0.0, fill_g: 0.0, fill_b: 0.0, fill_set: false,
            path_start: None,
            segments: Vec::new(),
        }
    }
}

// ===== Text Chunk =====
#[derive(Debug, Clone)]
struct TextChunk {
    text: String,
    x: f64,
    y: f64,
    font_size: f64,
    h_scale: f64,
    font_name: String,
    page: usize,
    color: String,  // CSS color e.g. "#ff0000" or empty for default
}

// ===== Font Info =====
#[derive(Clone)]
struct FontInfo {
    base_font: String,
    encoding: String,     // e.g. "WinAnsiEncoding", "MacRomanEncoding"
    widths: Option<Vec<i64>>,
    first_char: i64,
    to_unicode: Option<Vec<(u32, String)>>, // character code → unicode mapping
}

// ===== Page Content =====
struct PageContent {
    items: Vec<TextChunk>,
    width: f64,
    height: f64,
    images: Vec<ImageInfo>,
    links: Vec<LinkInfo>,
    tables: Vec<TableInfo>,
}

struct ImageInfo {
    x: f64, y: f64, w: f64, h: f64,
    data: Vec<u8>,
    mime: String,
}

struct LinkInfo {
    x: f64, y: f64, w: f64, h: f64,
    uri: String,
}

// ===== Main Converter =====

/// Convert a PDF file to an HTML string for rendering

pub fn pdf_to_html(path: &str) -> Result<String, String> {
    let doc = Document::load(path).map_err(|e| format!("无法加载PDF: {}", e))?;
    let pages = doc.get_pages();
    let mut all_pages: Vec<PageContent> = Vec::new();

    for (page_num, page_id) in &pages {
        match extract_page(&doc, *page_id, *page_num as usize) {
            Ok(pc) => all_pages.push(pc),
            Err(e) => eprintln!("[pdf2html] 页面{}跳过: {}", page_num, e),
        }
    }

    if all_pages.is_empty() {
        return Err("未能提取任何页面内容".into());
    }

    Ok(generate_html(&all_pages))
}

// ===== Page Extraction =====

fn extract_page(doc: &Document, page_id: ObjectId, page_num: usize) -> Result<PageContent, String> {
    let page_dict = doc.get_dictionary(page_id).map_err(|e| format!("获取页面失败: {}", e))?;

    // Get page dimensions (MediaBox)
    let media_box = page_dict.get(b"MediaBox")
        .ok()
        .and_then(|o| o.as_array().ok());

    let (width, height) = if let Some(mb) = media_box {
        let values: Vec<f64> = mb.iter()
            .filter_map(|o| match o {
                Object::Integer(val) => Some(*val as f64),
                Object::Real(val) => Some(*val as f64),
                _ => None,
            })
            .collect();
        if values.len() >= 4 {
            (values[2] - values[0], values[3] - values[1])
        } else {
            (612.0, 792.0)
        }
    } else {
        (612.0, 792.0)
    };
    let (width, height) = if let Some(mb) = media_box {
        let values: Vec<f64> = mb.iter()
            .filter_map(|o| match o {
                Object::Integer(i) => Some(*i as f64),
                Object::Real(r) => Some((*r) as f64),
                _ => None,
            })
            .collect();
        if values.len() >= 4 {
            (values[2] - values[0], values[3] - values[1])
        } else {
            (612.0, 792.0) // US Letter default
        }
    } else {
        (612.0, 792.0)
    };

    // Get page resources (fonts, xobjects, etc.)
    // Resolve Resources (may be a reference to shared resources dict)
    let resources_val = page_dict.get(b"Resources").ok();
    let resources = resources_val.and_then(|o| {
        if let Ok(ref_id) = o.as_reference() {
            doc.get_object(ref_id).ok().and_then(|o| o.as_dict().ok())
        } else {
            o.as_dict().ok()
        }
    });

    // Get content streams
    // content already loaded
    // Get page resources
    // Resolve Resources (may be a reference to shared resources dict)
    let resources_val = page_dict.get(b"Resources").ok();
    let resources = resources_val.and_then(|o| {
        if let Ok(ref_id) = o.as_reference() {
            doc.get_object(ref_id).ok().and_then(|o| o.as_dict().ok())
        } else {
            o.as_dict().ok()
        }
    });

    // Get content streams using lopdf 0.44 API
    let content = doc.get_page_content(page_id);
    
    // Extract fonts from page resources
    let fonts = extract_fonts(doc, &resources);
    
    // Parse and interpret content stream
    let operator_handler = OperatorHandler::new(doc, &fonts, width, height);
    let items = operator_handler.process(&content, page_num, &resources);
    
    let images = extract_images(doc, &resources);
    let links = extract_links(doc, &page_dict, height);
    Ok(PageContent {
        items,
        width,
        height,
        images,
        links,
        tables: Vec::new(),
    })
}

// ===== Font Extraction =====

fn extract_fonts(doc: &Document, resources: &Option<&lopdf::Dictionary>) -> HashMap<String, FontInfo> {
    let mut fonts: HashMap<String, FontInfo> = HashMap::new();
    
    let Some(res) = resources else { return fonts };
    let Ok(font_dict) = res.get(b"Font").and_then(|o| o.as_dict()) else { return fonts };
    
    for (key, val) in font_dict.iter() {
        let font_name = String::from_utf8_lossy(key).trim_start_matches('/').to_string();
        let obj = if let Ok(ref_id) = val.as_reference() {
            doc.get_object(ref_id).ok().map(|o| o.clone())
        } else {
            Some(val.clone())
        };
        
        let Some(obj) = obj else { continue };
        let Some(dict) = obj.as_dict().ok() else { continue };
        
        let base_font = dict.get(b"BaseFont")
            .and_then(|o| Ok(String::from_utf8_lossy(o.as_name()?).to_string()))
            .unwrap_or_default();
        
        let encoding = dict.get(b"Encoding")
            .and_then(|o| Ok(String::from_utf8_lossy(o.as_name()?).to_string()))
            .unwrap_or_default();
        
        // Try to get ToUnicode CMap
        let to_unicode = dict.get(b"ToUnicode").ok()
            .and_then(|o| {
                if let Ok(ref_id) = o.as_reference() {
                    doc.get_dictionary(ref_id).ok()
                        .and_then(|d| d.get(b"Length").ok())
                        .and_then(|_| {
                            doc.get_object(ref_id).ok()
                                .and_then(|obj| obj.as_stream().ok())
                                .and_then(|s| s.decompressed_content().ok())
                        })
                } else {
                    o.as_stream().ok()
                        .and_then(|s| s.decompressed_content().ok())
                }
            });

        let unicode_map = to_unicode.as_deref().and_then(|data| {
            parse_cmap(data)
        });

        let widths = dict.get(b"Widths").ok().and_then(|o| o.as_array().ok()).map(|arr| {
            arr.iter().filter_map(|o| o.as_i64().ok()).collect::<Vec<i64>>()
        });
        let first_char = dict.get(b"FirstChar").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0);

        fonts.insert(font_name, FontInfo {
            base_font,
            encoding,
            widths,
            first_char,
            to_unicode: unicode_map.map(|v| v.into_iter().map(|(c, s)| (c as u32, s)).collect()),
        });
    }
    
    fonts
}

/// Enhanced CMap parser — handles multi-byte CID, codespacerange, Type0
fn parse_cmap(data: &[u8]) -> Option<Vec<(u32, String)>> {
    let text = String::from_utf8_lossy(data);
    let mut map: Vec<(u32, String)> = Vec::new();
    
    // Parse beginbfchar...endbfchar
    if let Some(start) = text.find("beginbfchar") {
        let end = text[start..].find("endbfchar").map(|p| start + p).unwrap_or(text.len());
        let block = &text[start+11..end];
        for line in block.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let src = parse_pdf_hex(parts[0]).unwrap_or(0) as u32;
                let dst = if parts[1].starts_with('<') {
                    let hex = &parts[1][1..parts[1].len()-1];
                    let bytes = hex_to_bytes(hex);
                    if bytes.len() == 2 {
                        let code = ((bytes[0] as u32) << 8) | (bytes[1] as u32);
                        char::from_u32(code).map(|c| c.to_string()).unwrap_or_default()
                    } else {
                        let chars: Vec<u16> = bytes.chunks(2).map(|c| ((c[0] as u16) << 8) | (c[1] as u16)).collect();
                        String::from_utf16(&chars).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).to_string())
                    }
                } else {
                    parts[1].replace('/', "")
                };
                if !dst.is_empty() {
                    map.push((src, dst));
                }
            }
        }
    }
    
    // Parse beginbfrange...endbfrange
    if let Some(start) = text.find("beginbfrange") {
        let end = text[start..].find("endbfrange").map(|p| start + p).unwrap_or(text.len());
        let block = &text[start+12..end];
        for line in block.lines() {
            let line = line.trim();
            if line.is_empty() { continue; }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 3 {
                let lo = parse_pdf_hex(parts[0]).unwrap_or(0) as u32;
                let hi = parse_pdf_hex(parts[1]).unwrap_or(lo) as u32;
                let dst_start = parts[2];
                if dst_start.starts_with('<') {
                    let hex = &dst_start[1..dst_start.len()-1];
                    let bytes = hex_to_bytes(hex);
                    for i in 0..=(hi - lo) {
                        let dst = if bytes.len() == 2 {
                            let base = ((bytes[0] as u32) << 8) | (bytes[1] as u32);
                            char::from_u32(base + i).map(|c| c.to_string()).unwrap_or_default()
                        } else {
                            String::from_utf8_lossy(&bytes).to_string()
                        };
                        if !dst.is_empty() { map.push((lo + i, dst)); }
                    }
                } else if dst_start.starts_with('[') {
                    let mut all = dst_start.to_string();
                    for p in &parts[3..] { all.push(' '); all.push_str(p); }
                    let arr_str = all.trim_start_matches('[').trim_end_matches(']').to_string();
                    let content = arr_str.as_str();
                    let mut idx = 0u32;
                    let mut pos = 0usize;
                    let chars: Vec<char> = content.chars().collect();
                    while pos < chars.len() {
                        if chars[pos] == '<' {
                            let end_idx = pos + 1 + content[pos+1..].find('>').unwrap_or(content.len() - pos - 1);
                            let hex_str: String = content[pos+1..end_idx].chars().collect();
                            let bytes = hex_to_bytes(&hex_str);
                            let dst = if bytes.len() == 2 {
                                let code = ((bytes[0] as u32) << 8) | (bytes[1] as u32);
                                char::from_u32(code).map(|c| c.to_string()).unwrap_or_default()
                            } else {
                                String::from_utf8_lossy(&bytes).to_string()
                            };
                            if !dst.is_empty() && lo + idx <= hi {
                                map.push((lo + idx, dst));
                            }
                            idx += 1;
                            pos = end_idx + 1;
                        } else {
                            pos += 1;
                        }
                        if idx > (hi - lo) { break; }
                    }
                }
            }
        }
    }
    
    if map.is_empty() { None } else { Some(map) }
}


fn parse_pdf_hex(s: &str) -> Option<u32> {
    let s = s.trim().trim_start_matches('<').trim_end_matches('>');
    u32::from_str_radix(s, 16).ok()
}

fn hex_to_bytes(s: &str) -> Vec<u8> {
    let s = s.trim();
    let mut bytes = Vec::with_capacity(s.len() / 2);
    let chars: Vec<char> = s.chars().collect();
    for chunk in chars.chunks(2) {
        let hex: String = chunk.iter().collect();
        if let Ok(b) = u8::from_str_radix(&hex, 16) {
            bytes.push(b);
        }
    }
    bytes
}

// ===== Content Stream Operator Handler =====


// ===== Font Encoding Tables =====

/// WinAnsiEncoding (code page 1252) character to Unicode mapping

fn decode_winansi(b: u8) -> char {
    if b <= 0x7E { return b as char; }
    match b {
        0xA0 => '\u{00a0}', 0xA1 => '\u{00a1}', 0xA2 => '\u{00a2}', 0xA3 => '\u{00a3}',
        0xA4 => '\u{00a4}', 0xA5 => '\u{00a5}', 0xA6 => '\u{00a6}', 0xA7 => '\u{00a7}',
        0xA8 => '\u{00a8}', 0xA9 => '\u{00a9}', 0xAA => '\u{00aa}', 0xAB => '\u{00ab}',
        0xAC => '\u{00ac}', 0xAD => '\u{00ad}', 0xAE => '\u{00ae}', 0xAF => '\u{00af}',
        0xB0 => '\u{00b0}', 0xB1 => '\u{00b1}', 0xB2 => '\u{00b2}', 0xB3 => '\u{00b3}',
        0xB4 => '\u{00b4}', 0xB5 => '\u{00b5}', 0xB6 => '\u{00b6}', 0xB7 => '\u{00b7}',
        0xB8 => '\u{00b8}', 0xB9 => '\u{00b9}', 0xBA => '\u{00ba}', 0xBB => '\u{00bb}',
        0xBC => '\u{00bc}', 0xBD => '\u{00bd}', 0xBE => '\u{00be}', 0xBF => '\u{00bf}',
        0xC0 => '\u{00c0}', 0xC1 => '\u{00c1}', 0xC2 => '\u{00c2}', 0xC3 => '\u{00c3}',
        0xC4 => '\u{00c4}', 0xC5 => '\u{00c5}', 0xC6 => '\u{00c6}', 0xC7 => '\u{00c7}',
        0xC8 => '\u{00c8}', 0xC9 => '\u{00c9}', 0xCA => '\u{00ca}', 0xCB => '\u{00cb}',
        0xCC => '\u{00cc}', 0xCD => '\u{00cd}', 0xCE => '\u{00ce}', 0xCF => '\u{00cf}',
        0xD0 => '\u{00d0}', 0xD1 => '\u{00d1}', 0xD2 => '\u{00d2}', 0xD3 => '\u{00d3}',
        0xD4 => '\u{00d4}', 0xD5 => '\u{00d5}', 0xD6 => '\u{00d6}', 0xD7 => '\u{00d7}',
        0xD8 => '\u{00d8}', 0xD9 => '\u{00d9}', 0xDA => '\u{00da}', 0xDB => '\u{00db}',
        0xDC => '\u{00dc}', 0xDD => '\u{00dd}', 0xDE => '\u{00de}', 0xDF => '\u{00df}',
        0xE0 => '\u{00e0}', 0xE1 => '\u{00e1}', 0xE2 => '\u{00e2}', 0xE3 => '\u{00e3}',
        0xE4 => '\u{00e4}', 0xE5 => '\u{00e5}', 0xE6 => '\u{00e6}', 0xE7 => '\u{00e7}',
        0xE8 => '\u{00e8}', 0xE9 => '\u{00e9}', 0xEA => '\u{00ea}', 0xEB => '\u{00eb}',
        0xEC => '\u{00ec}', 0xED => '\u{00ed}', 0xEE => '\u{00ee}', 0xEF => '\u{00ef}',
        0xF0 => '\u{00f0}', 0xF1 => '\u{00f1}', 0xF2 => '\u{00f2}', 0xF3 => '\u{00f3}',
        0xF4 => '\u{00f4}', 0xF5 => '\u{00f5}', 0xF6 => '\u{00f6}', 0xF7 => '\u{00f7}',
        0xF8 => '\u{00f8}', 0xF9 => '\u{00f9}', 0xFA => '\u{00fa}', 0xFB => '\u{00fb}',
        0xFC => '\u{00fc}', 0xFD => '\u{00fd}', 0xFE => '\u{00fe}', 0xFF => '\u{00ff}',
        _ => '\u{fffd}',
    }
}

fn decode_macroman(b: u8) -> char {
    if b <= 0x7E { return b as char; }
    match b {
        0xA0 => '\u{00a0}', 0xA1 => '\u{00a1}', 0xA2 => '\u{00a2}', 0xA3 => '\u{00a3}',
        0xA4 => '\u{00a4}', 0xA5 => '\u{00a5}', 0xA6 => '\u{00a6}', 0xA7 => '\u{00a7}',
        0xA8 => '\u{00a8}', 0xA9 => '\u{00a9}', 0xAA => '\u{00aa}', 0xAB => '\u{00ab}',
        0xAC => '\u{00ac}', 0xAD => '\u{00ad}', 0xAE => '\u{00ae}', 0xAF => '\u{00af}',
        0xB0 => '\u{00b0}', 0xB1 => '\u{00b1}', 0xB2 => '\u{00b2}', 0xB3 => '\u{00b3}',
        0xB4 => '\u{00b4}', 0xB5 => '\u{00b5}', 0xB6 => '\u{00b6}', 0xB7 => '\u{00b7}',
        0xB8 => '\u{00b8}', 0xB9 => '\u{00b9}', 0xBA => '\u{00ba}', 0xBB => '\u{00bb}',
        0xBC => '\u{00bc}', 0xBD => '\u{00bd}', 0xBE => '\u{00be}', 0xBF => '\u{00bf}',
        0xC0 => '\u{00c0}', 0xC1 => '\u{00c1}', 0xC2 => '\u{00c2}', 0xC3 => '\u{00c3}',
        0xC4 => '\u{00c4}', 0xC5 => '\u{00c5}', 0xC6 => '\u{00c6}', 0xC7 => '\u{00c7}',
        0xC8 => '\u{00c8}', 0xC9 => '\u{00c9}', 0xCA => '\u{00ca}', 0xCB => '\u{00cb}',
        0xCC => '\u{00cc}', 0xCD => '\u{00cd}', 0xCE => '\u{00ce}', 0xCF => '\u{00cf}',
        0xD0 => '\u{00d0}', 0xD1 => '\u{00d1}', 0xD2 => '\u{00d2}', 0xD3 => '\u{00d3}',
        0xD4 => '\u{00d4}', 0xD5 => '\u{00d5}', 0xD6 => '\u{00d6}', 0xD7 => '\u{00d7}',
        0xD8 => '\u{00d8}', 0xD9 => '\u{00d9}', 0xDA => '\u{00da}', 0xDB => '\u{00db}',
        0xDC => '\u{00dc}', 0xDD => '\u{00dd}', 0xDE => '\u{00de}', 0xDF => '\u{00df}',
        0xE0 => '\u{00e0}', 0xE1 => '\u{00e1}', 0xE2 => '\u{00e2}', 0xE3 => '\u{00e3}',
        0xE4 => '\u{00e4}', 0xE5 => '\u{00e5}', 0xE6 => '\u{00e6}', 0xE7 => '\u{00e7}',
        0xE8 => '\u{00e8}', 0xE9 => '\u{00e9}', 0xEA => '\u{00ea}', 0xEB => '\u{00eb}',
        0xEC => '\u{00ec}', 0xED => '\u{00ed}', 0xEE => '\u{00ee}', 0xEF => '\u{00ef}',
        0xF0 => '\u{00f0}', 0xF1 => '\u{00f1}', 0xF2 => '\u{00f2}', 0xF3 => '\u{00f3}',
        0xF4 => '\u{00f4}', 0xF5 => '\u{00f5}', 0xF6 => '\u{00f6}', 0xF7 => '\u{00f7}',
        0xF8 => '\u{00f8}', 0xF9 => '\u{00f9}', 0xFA => '\u{00fa}', 0xFB => '\u{00fb}',
        0xFC => '\u{00fc}', 0xFD => '\u{00fd}', 0xFE => '\u{00fe}', 0xFF => '\u{00ff}',
        _ => '\u{fffd}',
    }
}
fn decode_byte(b: u8, encoding: &str) -> char {
    if encoding.is_empty() || encoding == "Identity-H" || encoding == "Identity-V" {
        // Identity-H: treat byte as Unicode code point (or Latin-1 fallback)
        char::from_u32(b as u32).unwrap_or('\u{fffd}')
    } else if encoding == "MacRomanEncoding" {
        decode_macroman(b)
    } else {
        // Default: WinAnsiEncoding (also covers "WinAnsiEncoding")
        decode_winansi(b)
    }
}
struct OperatorHandler<'a> {
    doc: &'a Document,
    fonts: &'a HashMap<String, FontInfo>,
    page_width: f64,
    page_height: f64,
    xobjects: Option<&'a lopdf::Dictionary>,
}

impl<'a> OperatorHandler<'a> {
    fn new(doc: &'a Document, fonts: &'a HashMap<String, FontInfo>, 
           pw: f64, ph: f64) -> Self {
        Self { doc, fonts, page_width: pw, page_height: ph, xobjects: None }
    }

    fn process(&self, data: &[u8], page_num: usize, 
               resources: &Option<&lopdf::Dictionary>) -> Vec<TextChunk> {
        let mut chunks = Vec::new();
        
        // Parse content stream using lopdf's Content parser
        let content = match Content::decode(data) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[pdf2html] 内容流解析失败: {}", e);
                return chunks;
            }
        };

        let mut state = GraphicsState::default();
        let mut stack: Vec<GraphicsState> = Vec::new();

        for op in &content.operations {
            if let Err(e) = self.handle_operation(op, &mut state, &mut stack, &mut chunks, page_num) {
                eprintln!("[pdf2html] 操作符跳过 `{}': {}", op.operator, e);
            }
        }

        chunks
    }

    fn handle_operation(
        &self,
        op: &Operation,
        state: &mut GraphicsState,
        stack: &mut Vec<GraphicsState>,
        chunks: &mut Vec<TextChunk>,
        page_num: usize,
    ) -> Result<(), String> {
        match op.operator.as_str() {
            // ===== Graphics State =====
            "q" => {
                stack.push(state.clone());
                Ok(())
            }
            "Q" => {
                if let Some(saved) = stack.pop() {
                    *state = saved;
                }
                Ok(())
            }
            "cm" => {
                let nums = self.parse_numbers(&op.operands, 6)?;
                state.ctm = multiply_matrix(&state.ctm, &nums);
                Ok(())
            }

            // ===== Text State =====
            "BT" => {
                state.in_text = true;
                state.text_matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                state.text_line_matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
                Ok(())
            }
            "ET" => {
                state.in_text = false;
                Ok(())
            }
            "Tf" => {
                let name = op.operands.first()
                    .and_then(|o| o.as_name().ok())
                    .ok_or_else(|| "Tf缺少字体名".to_string())?;
                let size = op.operands.get(1)
                    .and_then(|o| o.as_f32().ok().map(|v| v as f64)
                        .or_else(|| o.as_i64().ok().map(|v| v as f64)))
                    .ok_or_else(|| "Tf缺少字号".to_string())?;
                state.text.font_name = String::from_utf8_lossy(name).to_string();
                state.text.font_size = size as f64;
                Ok(())
            }
            "Td" => {
                let nums = self.parse_numbers(&op.operands, 2)?;
                // Translate text matrix and text line matrix
                state.text_matrix = multiply_matrix(&state.text_matrix, &[1.0, 0.0, 0.0, 1.0, nums[0], nums[1]]);
                state.text_line_matrix = state.text_matrix;
                Ok(())
            }
            "TD" => {
                let nums = self.parse_numbers(&op.operands, 2)?;
                state.text.leading = -nums[1];
                state.text_matrix = multiply_matrix(&state.text_matrix, &[1.0, 0.0, 0.0, 1.0, nums[0], nums[1]]);
                state.text_line_matrix = state.text_matrix;
                Ok(())
            }
            "Tm" => {
                let nums = self.parse_numbers(&op.operands, 6)?;
                state.text_matrix = nums;
                state.text_line_matrix = nums;
                Ok(())
            }
            "T*" => {
                state.text_matrix = multiply_matrix(&state.text_line_matrix, &[1.0, 0.0, 0.0, 1.0, 0.0, -state.text.leading]);
                state.text_line_matrix = state.text_matrix;
                Ok(())
            }
            "Ts" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                state.text.rise = nums[0];
                Ok(())
            }
            "Tc" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                state.text.char_spacing = nums[0];
                Ok(())
            }
            "Tw" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                state.text.word_spacing = nums[0];
                Ok(())
            }
            "Tz" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                state.text.h_scale = nums[0];
                Ok(())
            }
            "TL" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                state.text.leading = nums[0];
                Ok(())
            }

            // ===== Text Showing =====
            "Tj" => {
                let raw = self.extract_text_bytes(&op.operands)?;
                let decoded = self.decode_text(&raw, state);
                if !decoded.is_empty() {
                    let pos = self.text_position(state);
                    let css_color = self.state_color(state);
                    chunks.push(TextChunk {
                        text: decoded,
                        x: pos.0, y: pos.1,
                        font_size: (state.text.font_size * state.ctm[0]).max(1.0),
                        h_scale: state.text.h_scale,
                        font_name: self.fonts.get(&state.text.font_name)
                            .map(|f| f.base_font.clone())
                            .unwrap_or_else(|| state.text.font_name.clone()),
                        page: page_num,
                        color: css_color,
                    });
                }
                // Advance text matrix
                let width = self.text_width(&raw, state);
                self.advance_text_matrix(state, width);
                Ok(())
            }
            "'" => {
                // Move to next line and show text
                state.text_matrix = multiply_matrix(&state.text_line_matrix, &[1.0, 0.0, 0.0, 1.0, 0.0, -state.text.leading]);
                state.text_line_matrix = state.text_matrix;
                
                let raw = self.extract_text_bytes(&op.operands)?;
                let decoded = self.decode_text(&raw, state);
                if !decoded.is_empty() {
                    let pos = self.text_position(state);
                    let css_color = self.state_color(state);
                    chunks.push(TextChunk {
                        text: decoded,
                        x: pos.0, y: pos.1,
                        font_size: (state.text.font_size * state.ctm[0]).max(1.0),
                        h_scale: state.text.h_scale,
                        font_name: self.fonts.get(&state.text.font_name)
                            .map(|f| f.base_font.clone())
                            .unwrap_or_else(|| state.text.font_name.clone()),
                        page: page_num,
                        color: css_color,
                    });
                }
                let width = self.text_width(&raw, state);
                self.advance_text_matrix(state, width);
                Ok(())
            }
            "\"" => {
                // Set word/char spacing, move to next line, show text
                let nums = self.parse_numbers(&op.operands, 2)?;
                state.text.word_spacing = nums[0];
                state.text.char_spacing = nums[1];
                
                state.text_matrix = multiply_matrix(&state.text_line_matrix, &[1.0, 0.0, 0.0, 1.0, 0.0, -state.text.leading]);
                state.text_line_matrix = state.text_matrix;
                
                // The string is the 3rd operand but we only parsed 2 numbers
                // Let the rest of the operands be the string
                if let Some(str_op) = op.operands.get(2) {
                    if let Ok(raw) = self.extract_bytes_from_op(str_op) {
                        let decoded = self.decode_text(&raw, state);
                        if !decoded.is_empty() {
                            let pos = self.text_position(state);
                            let css_color = self.state_color(state);
                            chunks.push(TextChunk {
                                text: decoded,
                                x: pos.0, y: pos.1,
                                font_size: (state.text.font_size * state.ctm[0]).max(1.0),
                                h_scale: state.text.h_scale,
                                font_name: state.text.font_name.clone(),
                                page: page_num,
                                color: css_color,
                            });
                        }
                        let width = self.text_width(&raw, state);
                        self.advance_text_matrix(state, width);
                    }
                }
                Ok(())
            }
            "TJ" => {
                // Array of strings and numbers (for kerning) per PDF 32000-1:2008 §9.4.5
                if let Some(arr) = op.operands.first().and_then(|o| o.as_array().ok()) {
                    for item in arr.iter() {
                        if let Ok(bytes) = item.as_str() {
                            // String: show text at current position, then advance
                            let decoded = self.decode_text(bytes, state);
                            if !decoded.is_empty() {
                                let pos = self.text_position(state);
                                let css_color = self.state_color(state);
                                chunks.push(TextChunk {
                                    text: decoded,
                                    x: pos.0, y: pos.1,
                                    font_size: (state.text.font_size * state.ctm[0]).max(1.0),
                                    h_scale: state.text.h_scale,
                                    font_name: state.text.font_name.clone(),
                                    page: page_num,
                                    color: css_color,
                                });
                            }
                            let char_count = bytes.len().max(1);
                            let width = char_count as f64 * state.text.font_size * 0.5;
                            let spacing = state.text.char_spacing * state.text.font_size;
                            let total_width = width + spacing * char_count as f64;
                            self.advance_text_matrix(state, total_width);
                        } else if let Ok(num) = item.as_i64().or_else(|_| item.as_f32().map(|f| f as i64)) {
                            // Number: kerning offset (thousandths of a unit of text space)
                            // Positive = reduce space (move left), Negative = increase space (move right)
                            if num != 0 {
                                let offset = -(num as f64) / 1000.0 * state.text.font_size * state.text.h_scale / 100.0;
                                self.advance_text_matrix(state, offset);
                            }
                        }
                    }
                }
                Ok(())
            }

            // ===== Marked Content (skip) =====
            "BMC" | "BDC" | "EMC" | "MP" | "DP" => Ok(()),

            // ===== XObject (images) =====
            "Do" => {
                // Skip images for now
                Ok(())
            }

            // ===== Path construction (skip - we only care about text) =====
            "c" | "v" | "y" | "f" | "F" | "f*" | "B" | "B*" | "b" | "b*" | "n" => Ok(()),
            "W" | "W*" => Ok(()),
            
            // ===== Color =====
            "rg" => {
                let arr = &op.operands;
                    if arr.len() >= 3 {
                        if let (Ok(r), Ok(g), Ok(b)) = (
                            arr[0].as_f32().map(|v| v as f64).or_else(|_| arr[0].as_i64().map(|v| v as f64)),
                            arr[1].as_f32().map(|v| v as f64).or_else(|_| arr[1].as_i64().map(|v| v as f64)),
                            arr[2].as_f32().map(|v| v as f64).or_else(|_| arr[2].as_i64().map(|v| v as f64)),
                        ) {
                            state.fill_r = r.clamp(0.0, 1.0);
                            state.fill_g = g.clamp(0.0, 1.0);
                            state.fill_b = b.clamp(0.0, 1.0);
                            state.fill_set = true;
                        }
                    }
                Ok(())
            }
            "RG" => {
                // Same as rg for stroke color; for simplicity, treat as fill for text
                let arr = &op.operands;
                    if arr.len() >= 3 {
                        if let (Ok(r), Ok(g), Ok(b)) = (
                            arr[0].as_f32().map(|v| v as f64).or_else(|_| arr[0].as_i64().map(|v| v as f64)),
                            arr[1].as_f32().map(|v| v as f64).or_else(|_| arr[1].as_i64().map(|v| v as f64)),
                            arr[2].as_f32().map(|v| v as f64).or_else(|_| arr[2].as_i64().map(|v| v as f64)),
                        ) {
                            state.fill_r = r.clamp(0.0, 1.0);
                            state.fill_g = g.clamp(0.0, 1.0);
                            state.fill_b = b.clamp(0.0, 1.0);
                            state.fill_set = true;
                        }
                    }
                Ok(())
            }
            "g" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                let gray = (nums[0] / 100.0).clamp(0.0, 1.0);
                state.fill_r = gray; state.fill_g = gray; state.fill_b = gray; state.fill_set = true;
                Ok(())
            }
            "G" => {
                let nums = self.parse_numbers(&op.operands, 1)?;
                let gray = (nums[0] / 100.0).clamp(0.0, 1.0);
                state.fill_r = gray; state.fill_g = gray; state.fill_b = gray; state.fill_set = true;
                Ok(())
            }
            "k" | "K" => {
                // CMYK -> RGB (simple conversion)
                let nums = self.parse_numbers(&op.operands, 4)?;
                let c = nums[0]; let m = nums[1]; let y = nums[2]; let k = nums[3];
                let r = (1.0 - c) * (1.0 - k);
                let g = (1.0 - m) * (1.0 - k);
                let b = (1.0 - y) * (1.0 - k);
                state.fill_r = r.clamp(0.0, 1.0);
                state.fill_g = g.clamp(0.0, 1.0);
                state.fill_b = b.clamp(0.0, 1.0);
                state.fill_set = true;
                Ok(())
            }
            "CS" | "cs" | "SC" | "sc" | "SCN" | "scn" => {
                // Non-RGB color spaces: skip for now
                Ok(())
            }
            

            // ===== Path Operators (table detection) =====
            "re" => {
                let nums = self.parse_numbers(&op.operands, 4)?;
                let (x, y, w, h) = (nums[0], nums[1], nums[2], nums[3]);
                state.segments.push(PathSegment{x1:x, y1:y, x2:x+w, y2:y});
                state.segments.push(PathSegment{x1:x+w, y1:y, x2:x+w, y2:y+h});
                state.segments.push(PathSegment{x1:x+w, y1:y+h, x2:x, y2:y+h});
                state.segments.push(PathSegment{x1:x, y1:y+h, x2:x, y2:y});
                Ok(())
            }
            "m" => {
                let nums = self.parse_numbers(&op.operands, 2)?;
                state.path_start = Some((nums[0], nums[1]));
                Ok(())
            }
            "l" => {
                if let Some((cx, cy)) = state.path_start {
                    let nums = self.parse_numbers(&op.operands, 2)?;
                    state.segments.push(PathSegment{x1:cx, y1:cy, x2:nums[0], y2:nums[1]});
                    state.path_start = Some((nums[0], nums[1]));
                }
                Ok(())
            }
            "h" => { Ok(()) }
            "S" | "s" => { Ok(()) }
            // ===== Other (ignore silently) =====
            _ => Ok(()),
        }
    }

    fn parse_numbers(&self, ops: &[Object], count: usize) -> Result<[f64; 6], String> {
        let mut nums = [0.0f64; 6];
        for i in 0..count.min(6) {
            nums[i] = ops.get(i)
                .and_then(|o| o.as_i64().ok().map(|v| v as f64)
                    .or_else(|| o.as_f32().ok().map(|v| v as f64)))
                .ok_or_else(|| format!("缺少操作数[{}]", i))?;
        }
        Ok(nums)
    }

    fn extract_text_bytes(&self, ops: &[Object]) -> Result<Vec<u8>, String> {
        if let Some(obj) = ops.first() {
            self.extract_bytes_from_op(obj)
        } else {
            Ok(Vec::new())
        }
    }

    fn extract_bytes_from_op(&self, obj: &Object) -> Result<Vec<u8>, String> {
        if let Ok(bytes) = obj.as_str() {
            Ok(bytes.to_vec())
        } else if let Ok(name) = obj.as_name() {
            Ok(name.to_vec())
        } else {
            Err("无法提取文本字节".to_string())
        }
    }

    fn decode_text(&self, raw: &[u8], state: &GraphicsState) -> String {
        let font_name = &state.text.font_name;
        let font_info = self.fonts.get(font_name);
        
        // First try ToUnicode CMap
        if let Some(font) = font_info {
            if let Some(cmap) = &font.to_unicode {
                let mut result = String::new();
                if raw.len() == 1 {
                    let code = raw[0] as u16;
                    if let Some((_, ch)) = cmap.iter().find(|(c, _)| *c == code as u32) {
                        return ch.clone();
                    }
                } else if raw.len() == 2 {
                    let code = ((raw[0] as u16) << 8) | (raw[1] as u16);
                    if let Some((_, ch)) = cmap.iter().find(|(c, _)| *c == code as u32) {
                        return ch.clone();
                    }
                }
                // Multi-byte fallback: try each byte
                for &b in raw {
                    let code = b as u16;
                    if let Some((_, ch)) = cmap.iter().find(|(c, _)| *c == code as u32) {
                        result.push_str(ch);
                    } else {
                        result.push(decode_byte(b, &font.encoding));
                    }
                }
                return result;
            }
            
            // Try encoding-based decoding
            if !font.encoding.is_empty() && font.encoding != "Identity-H" {
                return raw.iter().map(|&b| decode_byte(b, &font.encoding)).collect();
            }
        }
        
        // Default: interpret bytes as Latin-1
        raw.iter().map(|&b| decode_byte(b, "")).collect::<String>()
    }

    fn text_width(&self, raw: &[u8], state: &GraphicsState) -> f64 {
        // Use font Widths array if available
        let font_info = self.fonts.get(&state.text.font_name);
        if let Some(font) = font_info {
            if let Some(widths) = &font.widths {
                let char_count = raw.len().max(1);
                let total_width: f64 = raw.iter().map(|&b| {
                    let idx = (b as i64) - font.first_char;
                    if idx >= 0 && (idx as usize) < widths.len() {
                        widths[idx as usize] as f64
                    } else {
                        500.0  // default width (0.5 * 1000)
                    }
                }).sum();
                let base_width = total_width * state.text.font_size / 1000.0;
                let spacing = state.text.char_spacing * state.text.font_size;
                return base_width + spacing * char_count as f64;
            }
        }
        // Fallback: rough width estimation
        let char_count = raw.len().max(1);
        let base_width = char_count as f64 * state.text.font_size * 0.5;
        let spacing = state.text.char_spacing * state.text.font_size;
        base_width + spacing * char_count as f64
    }

    fn advance_text_matrix(&self, state: &mut GraphicsState, width: f64) {
        // Advance Tm by (width * h_scale/100, 0)
        let tx = width * state.text.h_scale / 100.0;
        state.text_matrix[4] += tx * state.text_matrix[0] + 0.0 * state.text_matrix[2];
        state.text_matrix[5] += tx * state.text_matrix[1] + 0.0 * state.text_matrix[3];
    }

    fn state_color(&self, state: &GraphicsState) -> String {
        if !state.fill_set { return String::new(); }
        let r = (state.fill_r * 255.0).round() as u8;
        let g = (state.fill_g * 255.0).round() as u8;
        let b = (state.fill_b * 255.0).round() as u8;
        if r == 0 && g == 0 && b == 0 { return String::new(); } // default black
        format!("#{:02x}{:02x}{:02x}", r, g, b)
    }

    fn text_position(&self, state: &GraphicsState) -> (f64, f64) {
        // Transform text position through CTM
        let tm = &state.text_matrix;
        let ctm = &state.ctm;
        
        let x = tm[4] * ctm[0] + tm[5] * ctm[2] + ctm[4];
        let y = tm[4] * ctm[1] + tm[5] * ctm[3] + ctm[5];
        
        (x, y)
    }
}

// ===== Matrix Operations =====

fn multiply_matrix(a: &[f64; 6], b: &[f64; 6]) -> [f64; 6] {
    [
        a[0]*b[0] + a[1]*b[2],
        a[0]*b[1] + a[1]*b[3],
        a[2]*b[0] + a[3]*b[2],
        a[2]*b[1] + a[3]*b[3],
        a[4]*b[0] + a[5]*b[2] + b[4],
        a[4]*b[1] + a[5]*b[3] + b[5],
    ]
}

// ===== HTML Generation =====

fn extract_links(doc: &Document, page_dict: &lopdf::Dictionary, page_height: f64) -> Vec<LinkInfo> {
    let mut links = Vec::new();
    let Ok(annots) = page_dict.get(b"Annots").and_then(|o| o.as_array()) else { return links };
    for annot_ref in annots.iter() {
        let obj = if let Ok(ref_id) = annot_ref.as_reference() {
            doc.get_object(ref_id).ok().map(|o| o.clone())
        } else { Some(annot_ref.clone()) };
        let Some(obj) = obj else { continue };
        let Some(dict) = obj.as_dict().ok() else { continue };
        let subtype = dict.get(b"Subtype").ok().and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).to_string()).unwrap_or_default();
        if subtype != "Link" { continue; }
        let Ok(rect) = dict.get(b"Rect").and_then(|o| o.as_array()) else { continue };
        let coords: Vec<f64> = rect.iter().filter_map(|o| match o {
            Object::Integer(i) => Some(*i as f64),
            Object::Real(r) => Some(*r as f64), _ => None,
        }).collect();
        if coords.len() < 4 { continue; }
        let (x1, y1, x2, y2) = (coords[0], coords[1], coords[2], coords[3]);
        let (w, h) = ((x2 - x1).abs(), (y2 - y1).abs());
        if w < 1.0 || h < 1.0 { continue; }
        let uri = dict.get(b"A").ok().and_then(|a_obj| {
            let a_dict = a_obj.as_dict().ok()?;
            String::from_utf8_lossy(a_dict.get(b"URI").ok()?.as_str().ok()?).to_string().into()
        }).unwrap_or_default();
        if uri.is_empty() { continue; }
        links.push(LinkInfo { x: x1.min(x2), y: y1.min(y2), w, h, uri });
    }
    links
}


/// Extract image XObjects from page resources
fn extract_images(doc: &Document, resources: &Option<&lopdf::Dictionary>) -> Vec<ImageInfo> {
    let mut images = Vec::new();
    let Some(res) = resources else { return images };
    // Resolve XObject reference if needed
    let xobj_val = res.get(b"XObject").ok();
    let xobj_dict = xobj_val.and_then(|o| {
        if let Ok(ref_id) = o.as_reference() {
            doc.get_object(ref_id).ok().and_then(|o| o.as_dict().ok())
        } else {
            o.as_dict().ok()
        }
    });
    let Some(xobj_dict) = xobj_dict else { return images };
    
    for (key, val) in xobj_dict.iter() {
        let img_name = String::from_utf8_lossy(key).to_string();
        let stream = if let Ok(ref_id) = val.as_reference() {
            doc.get_object(ref_id).ok().and_then(|o| o.as_stream().ok().cloned())
        } else {
            val.as_stream().ok().cloned()
        };
        let Some(mut stream) = stream else { continue };
        
        // Check Subtype
        let subtype = stream.dict.get(b"Subtype").ok()
            .and_then(|o| o.as_name().ok())
            .map(|n| String::from_utf8_lossy(n).to_string())
            .unwrap_or_default();
        if subtype != "Image" { continue; }
        
        // Get image dimensions
        let width = stream.dict.get(b"Width").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0) as f64;
        let height = stream.dict.get(b"Height").ok().and_then(|o| o.as_i64().ok()).unwrap_or(0) as f64;
        if width <= 0.0 || height <= 0.0 { continue; }
        
        // Check if JPEG (DCTDecode filter)
        let filters = stream.dict.get(b"Filter");
        let is_jpeg = filters.map_or(false, |f| {
            if let Ok(name) = f.as_name() {
                name == b"DCTDecode"
            } else if let Ok(arr) = f.as_array() {
                arr.iter().any(|item| item.as_name().map_or(false, |n| n == b"DCTDecode"))
            } else { false }
        });
        
        // Decompress the stream if not JPEG
        let data = if is_jpeg {
            stream.content  // raw JPEG data
        } else {
            let _ = stream.decompress();
            stream.content
        };
        
        if data.is_empty() { continue; }
        
        let mime = if is_jpeg { "image/jpeg".to_string() } else { "image/png".to_string() };
        
        images.push(ImageInfo {
            x: 0.0, y: 0.0, w: width, h: height,
            data,
            mime,
        });
    }
    
    // Sort images by size (largest first) for display priority
    images.sort_by(|a, b| (b.w * b.h).partial_cmp(&(a.w * a.h)).unwrap_or(std::cmp::Ordering::Equal));
    images
}

fn base64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        result.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        result.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 { result.push(TABLE[((triple >> 6) & 0x3F) as usize] as char); } else { result.push('='); }
        if chunk.len() > 2 { result.push(TABLE[(triple & 0x3F) as usize] as char); } else { result.push('='); }
    }
    result
}


// ===== Table Detection =====

/// A line segment from PDF path operations
#[derive(Debug, Clone)]
struct PathSegment {
    x1: f64, y1: f64, x2: f64, y2: f64,
}

/// Detected table region with cells
#[derive(Debug, Clone)]
struct TableInfo {
    rows: usize,
    cols: usize,
    cells: HashMap<(usize, usize), String>,
}
fn generate_html(pages: &[PageContent]) -> String {
    let mut body = String::new();
    let mut style = String::from(
        ".pdf2html-page{position:relative;margin:12px auto;background:#fff;\
         box-shadow:0 2px 12px rgba(0,0,0,.15);overflow:visible;}\
         .pdf2html-page p{margin:0;padding:2px 0;line-height:1.4;}\
         .pdf2html-text{position:absolute;white-space:nowrap;}\
        ");
        // Build font-family CSS classes from all unique font names
        style.push_str(".pdf2html-page{");
        {
            let mut seen = std::collections::HashSet::new();
            for page in pages {
                for chunk in &page.items {
                    if seen.insert(&chunk.font_name) {
                        let display_name = chunk.font_name.replace('+', "").replace('-', " ");
                        style.push_str(&format!("--f{}:{};", chunk.font_name, display_name));
                    }
                }
            }
        }
        style.push_str("}");
;
    for (i, page) in pages.iter().enumerate() {
        let w_px = (page.width * PT_TO_PX).ceil() as i32;
        let h_px = (page.height * PT_TO_PX).ceil() as i32;
        let pp = PT_TO_PX;
        
        body.push_str("<div class='pdf2html-page' style='width:");
        body.push_str(&w_px.to_string());
        body.push_str("px;height:");
        body.push_str(&h_px.to_string());
        body.push_str("px;' id='page");
        body.push_str(&(i + 1).to_string());
        body.push_str("'>");
        
                let mut chunks: Vec<&TextChunk> = page.items.iter().collect();
        chunks.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap().then(a.x.partial_cmp(&b.x).unwrap()));
        
        let mut lines: Vec<Vec<&TextChunk>> = Vec::new();
        let mut current_line: Vec<&TextChunk> = Vec::new();
        let mut last_y: f64 = 0.0;
        
        for (idx, chunk) in chunks.iter().enumerate() {
            if idx == 0 {
                current_line.push(chunk);
                last_y = chunk.y;
            } else if (chunk.y - last_y).abs() < chunk.font_size * 0.5 {
                current_line.push(chunk);
            } else {
                if !current_line.is_empty() {
                    current_line.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
                    lines.push(current_line.clone());
                    current_line.clear();
                }
                current_line.push(chunk);
                last_y = chunk.y;
            }
        }
        if !current_line.is_empty() {
            current_line.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap());
            lines.push(current_line);
        }
        
        // Normalize x positions (subtract minimum to account for CTM offset)
        let min_x = lines.iter().flat_map(|l| l.iter().map(|c| c.x)).fold(f64::INFINITY, |a, b| a.min(b));
        
        for line in &lines {
            body.push_str("<p style='position:relative;margin:0;'>");
            // Merge adjacent chunks on same line with same font/size/y and close x gap
            let mut merged: Vec<Vec<&&TextChunk>> = Vec::new();
            let mut cur_group: Vec<&&TextChunk> = Vec::new();
            for chunk in line.iter() {
                if let Some(last) = cur_group.last() {
                    let same_font = last.font_name == chunk.font_name;
                    let same_size = (last.font_size - chunk.font_size).abs() < 0.5;
                    let same_y = (last.y - chunk.y).abs() < last.font_size * 0.4;
                    let prev_end = last.x + last.text.len() as f64 * last.font_size * 0.5;
                    let gap = chunk.x - prev_end;
                    if same_font && same_size && same_y && gap.abs() < last.font_size * 0.8 {
                        cur_group.push(chunk);
                        continue;
                    }
                }
                if !cur_group.is_empty() {
                    merged.push(std::mem::take(&mut cur_group));
                }
                cur_group.push(chunk);
            }
            if !cur_group.is_empty() { merged.push(cur_group); }
            
            for group in &merged {
                let chunk = group[0];
                let first_x = group.iter().map(|c| c.x).fold(f64::INFINITY, |a, b| a.min(b));
                let x_px = (first_x - min_x) * pp;
                let y_px = (page.height - chunk.y) * pp;
                let fs = chunk.font_size * pp;
                let text: String = group.iter().map(|c| c.text.as_str()).collect();
                body.push_str("<span class='pdf2html-text' style='left:");
                body.push_str(&format!("{:.2}", x_px));
                body.push_str("px;top:");
                body.push_str(&format!("{:.2}", y_px));
                body.push_str("px;font-size:");
                body.push_str(&format!("{:.2}", fs));
                body.push_str("px");
                if !chunk.color.is_empty() {
                    body.push_str(";color:");
                    body.push_str(&chunk.color);
                }
                body.push_str(";font-family:var(--f");
                body.push_str(&chunk.font_name);
                body.push_str(")");
                if (chunk.h_scale - 100.0).abs() > 0.1 {
                    body.push_str(&format!(";transform:scaleX({:.4});display:inline-block;", chunk.h_scale / 100.0));
                }
                body.push_str("'>");
                body.push_str(&esc_html(&chunk.text));
                body.push_str("</span>");
            }
            body.push_str("</p>");
        }
                // Render images embedded in the page
        for img in &page.images {
            let b64 = base64_encode(&img.data);
            let pp = PT_TO_PX;
            // If image is larger than page (raw pixel dimensions), scale to fit page
            let (iw, ih) = if img.w > page.width || img.h > page.height {
                let scale_x = page.width / img.w;
                let scale_y = page.height / img.h;
                let s = scale_x.min(scale_y);
                (img.w * s, img.h * s)
            } else {
                (img.w, img.h)
            };
            let ix = 0.0;  // full-page images start at page origin
            let iy = (page.height - ih) * pp;
            body.push_str(&format!("<img src='data:{};base64,{}' style='position:absolute;left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px;max-width:none;' />",
                img.mime, b64, ix * pp, iy, iw * pp, ih * pp));
        }
                // Render tables
        for table in &page.tables {
            body.push_str("<table style='border-collapse:collapse;position:absolute;left:0;top:0;width:");body.push_str(&format!("{:.1}", page.width * pp));
            body.push_str("px;height:");body.push_str(&format!("{:.1}", page.height * pp));
            body.push_str("px;'>");
            for row in 0..table.rows {
                body.push_str("<tr>");
                for col in 0..table.cols {
                    let key = (row, col);
                    let cell_text = table.cells.get(&key).map(|s| s.as_str()).unwrap_or("");
                    body.push_str("<td style='border:1px solid #aaa;padding:2px 6px;font-size:10px;vertical-align:top;'>");
                    body.push_str(&esc_html(cell_text));
                    body.push_str("</td>");
                }
                body.push_str("</tr>");
            }
            body.push_str("</table>");
        }
body.push_str("</div>");
    } format!(
        "<div class='pdf2html' style='background:#525659;padding:12px 0;'>
<style>{}</style>{}</div>",
        style, body
    )
}

fn esc_html(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace("\"", "&quot;")
}

// ===== Integration route handler =====

/// Convert a PDF file to HTML using the Rust content stream interpreter
pub fn handle_pdf2html(path: &str) -> Result<String, String> {
    pdf_to_html(path)
}
