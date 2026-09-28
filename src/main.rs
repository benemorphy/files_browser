// files_browser - Markdown 文件浏览器 (Rust)
// Usage: files_browser [port] [root_dir] [base_path]
//   port:      server port (default: 8899)
//   root_dir:  directory to serve (default: ./docs)
//   base_path: URL prefix for links, e.g. /docs (default: "")
//   支持相对路径和绝对路径
// Fixes:
//   - 左侧目录按字母排序 (locale-aware)
//   - 中文目录跳转正确
//   - 统一 UTF-8 编码
//   - LaTeX 数学公式渲染 (KaTeX)
//   - PDF 转 HTML 渲染 (lopdf内容流解释器)
mod pdf2html;
use pulldown_cmark::{html, Options, Parser};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::thread;

const DEFAULT_PORT: u16 = 8899;

const SUPPORTED_EXTS: &[&str] = &["md", "rs", "py", "toml", "lock", "json", "yaml", "yml", "txt", "ini", "cfg", "log", "c", "h", "ipynb", "tex", "js", "css", "png", "jpg", "jpeg", "gif", "svg", "pdf"];


fn is_image_ext(path: &str) -> bool {
    let e = path.rsplit('.').next().unwrap_or("").to_lowercase();
    matches!(e.as_str(), "png" | "jpg" | "jpeg" | "gif" | "svg" | "ico")
}

fn is_supported(p: &Path) -> bool {
    p.extension().and_then(|x| x.to_str()).map(|x| SUPPORTED_EXTS.contains(&x)).unwrap_or(false)
}

fn hljs_lang(ext: &str) -> &str {
    match ext {
        "rs" => "rust",
        "py" => "python",
        "lock" => "toml",
        "yml" => "yaml",
        "js" => "javascript",
        "ts" => "typescript",
        "sh" => "bash",
        "ps1" => "powershell",
        "log" => "plaintext",
        "c" => "c",
        "h" => "c",
        "ipynb" => "json",
        "tex" => "latex",
        _ => ext,
    }
}

const HTML_TPL: &str = r##"<!DOCTYPE html>
<html lang="zh-CN">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title} &mdash; MD Viewer</title>
<link rel="stylesheet" href="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/styles/atom-one-dark.min.css">
<link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.css">
<style>
*{{margin:0;padding:0;box-sizing:border-box;}}
body{{font:14px/1.6 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;color:#2c3e50;background:#fafafa;height:100vh;overflow:hidden;display:flex;flex-direction:column;}}
/* Menubar */
#menubar{{height:30px;background:#1a1a2e;display:flex;align-items:center;padding:0 8px;font-size:13px;user-select:none;flex-shrink:0;z-index:100;}}
.file-menu{{position:relative;}}
.file-menu>button{{background:none;border:none;color:#a8b2d1;cursor:pointer;padding:4px 12px;font-size:13px;border-radius:3px;}}
.file-menu>button:hover{{background:#e94560;color:#fff;}}
.file-menu .dropdown{{display:none;position:absolute;top:100%;left:0;background:#1a1a2e;border:1px solid #333;border-radius:4px;min-width:160px;z-index:1000;box-shadow:0 4px 12px rgba(0,0,0,.4);}}
.file-menu .dropdown.show{{display:block;}}
.file-menu .dropdown a{{display:block;padding:8px 16px;color:#a8b2d1;cursor:pointer;font-size:13px;text-decoration:none;}}
.file-menu .dropdown a:hover{{background:#e94560;color:#fff;}}
.file-menu .dropdown .divider{{border-top:1px solid #333;margin:4px 0;}}
#file-info{{color:#a8b2d1;margin-left:16px;font-size:12px;}}
#save-indicator{{color:#e94560;margin-left:8px;font-size:12px;display:none;}}
/* Workspace */
#workspace{{display:flex;flex:1;overflow:hidden;}}
/* Sidebar */
#sidebar{{width:200px;min-width:80px;max-width:400px;background:#1a1a2e;color:#eee;padding:12px 10px;overflow-y:auto;flex-shrink:0;}}
#sidebar a{{display:block;color:#a8b2d1;text-decoration:none;padding:2px 8px;border-radius:3px;font-size:12px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}}
#sidebar a:hover,#sidebar a.active{{background:#e94560;color:#fff;}}
#sidebar h3{{font-size:13px;color:#e94560;margin:14px 0 6px;text-transform:uppercase;letter-spacing:1px;}}
#sidebar h3:first-child{{margin-top:0;}}
#sidebar .parent-btn{{display:block;color:#e94560;font-weight:bold;font-size:13px;padding:6px 8px;margin-bottom:10px;border-bottom:1px solid #333;}}
/* Resizers */
.resizer{{width:4px;cursor:col-resize;background:#333;flex-shrink:0;position:relative;}}
.resizer:hover,.resizer.dragging{{background:#e94560;}}
/* Editor */
#editor-panel{{width:45%;min-width:150px;display:flex;flex-direction:column;flex-shrink:0;}}
#editor-header{{background:#2c2c3e;color:#a8b2d1;padding:4px 12px;font-size:12px;display:flex;justify-content:space-between;align-items:center;flex-shrink:0;}}
#editor-filename{{color:#e94560;font-weight:bold;}}
#editor-lang{{color:#888;font-size:11px;}}
#editor{{flex:1;background:#1e1e2e;color:#cdd6f4;border:none;padding:12px;font:14px/1.55 'Cascadia Code','JetBrains Mono','Fira Code',Consolas,'Courier New',monospace;resize:none;outline:none;tab-size:4;white-space:pre-wrap;word-wrap:break-word;}}
#editor::placeholder{{color:#555;}}
#editor:focus{{background:#1a1a28;}}
/* Preview */
#preview{{flex:1;overflow-y:auto;padding:24px 32px;background:#fff;min-width:150px;}}
#preview h1{{color:#1a1a2e;border-bottom:3px solid #e94560;padding-bottom:10px;margin:0 0 25px;}}
#preview h2{{color:#1a1a2e;border-bottom:1px solid #eee;padding-bottom:8px;margin:30px 0 15px;}}
#preview h3{{color:#1a1a2e;margin:25px 0 10px;}}
#preview h4{{color:#555;margin:20px 0 8px;}}
#preview p{{margin:12px 0;}}
#preview a{{color:#e94560;}}
#preview code{{color:#e94560;background:#f5f5f5;padding:2px 5px;border-radius:3px;font-size:0.9em;}}
#preview pre{{background:#1a1a2e;color:#abb2bf;padding:16px 20px;border-radius:8px;overflow-x:auto;margin:15px 0;}}
#preview pre code{{color:#abb2bf;background:transparent;padding:0;}}
#preview table{{border-collapse:collapse;width:100%;margin:15px 0;}}
#preview th,#preview td{{border:1px solid #ddd;padding:8px 12px;text-align:left;}}
#preview th{{background:#1a1a2e;color:#fff;font-weight:bold;}}
#preview tr:nth-child(even){{background:#f5f5f5;}}
#preview blockquote{{border-left:4px solid #e94560;margin:15px 0;padding:10px 20px;background:#f9f9f9;border-radius:0 4px 4px 0;}}
#preview img{{max-width:100%;border-radius:6px;margin:10px 0;}}
#preview hr{{border:none;border-top:1px solid #ddd;margin:25px 0;}}
#preview ul,#preview ol{{margin:10px 0;padding-left:25px;}}
/* TeX preview */
#preview .tex-preview{{background:#fafafa;}}
#preview .tex-source{{font-family:'Consolas','Courier New',monospace;font-size:13px;line-height:1.6;white-space:pre-wrap;word-break:break-word;color:#2c3e50;background:#f8f9fa;padding:16px 20px;border-radius:8px;border:1px solid #e0e0e0;}}
/* Rendered TeX styles */
#preview .tex-body{{font-family:'Georgia','Times New Roman',serif;font-size:14px;line-height:1.7;color:#1a1a2e;max-width:880px;margin:0 auto;padding:20px 40px;}}
#preview .tex-titlepage{{text-align:center;margin-bottom:40px;padding-bottom:30px;border-bottom:2px solid #ddd;}}
#preview .tex-title{{font-size:24px;font-weight:bold;margin-bottom:16px;line-height:1.3;}}
#preview .tex-author{{font-size:14px;color:#555;margin-bottom:8px;}}
#preview .tex-date{{font-size:12px;color:#888;}}
#preview .tex-section{{font-size:20px;font-weight:bold;margin:30px 0 15px;padding-bottom:8px;border-bottom:1px solid #e0e0e0;color:#1a1a2e;}}
#preview .tex-subsection{{font-size:17px;font-weight:bold;margin:25px 0 12px;color:#2c3e50;}}
#preview .tex-subsubsection{{font-size:15px;font-weight:bold;margin:20px 0 10px;color:#34495e;}}
#preview .tex-paragraph{{font-size:14px;font-weight:bold;margin:15px 0 8px;}}
#preview .tex-para{{margin:10px 0;text-align:justify;}}
#preview .tex-abstract{{background:#f8f9fa;border-left:3px solid #e94560;padding:15px 20px;margin:20px 0;border-radius:0 6px 6px 0;font-size:13px;line-height:1.6;}}
#preview .tex-cite{{color:#2980b9;font-size:12px;}}
#preview .tex-table{{border-collapse:collapse;margin:15px auto;font-size:12px;}}
#preview .tex-table td{{border:1px solid #ccc;padding:6px 12px;text-align:center;}}
#preview .tex-center{{text-align:center;}}
#preview li{{margin:4px 0;}}
#preview .hljs{{background:transparent;}}
/* PDF Viewer */
#pdf-viewer{{display:none;flex-direction:column;height:100%;background:#525659;}}
#preview:has(#pdf-viewer){{padding:0;background:#525659;}}
#pdf-toolbar{{background:#1a1a2e;color:#a8b2d1;padding:6px 12px;display:flex;align-items:center;gap:12px;flex-wrap:wrap;font-size:13px;flex-shrink:0;}}
#pdf-toolbar button{{background:#333;color:#fff;border:none;padding:4px 10px;border-radius:3px;cursor:pointer;font-size:12px;}}
#pdf-toolbar button:hover{{background:#e94560;}}
#pdf-toolbar select,#pdf-toolbar input[type=checkbox]{{cursor:pointer;}}
#pdf-filename{{color:#e94560;font-weight:bold;margin-right:auto;}}
#pdf-canvas{{margin:12px auto;display:block;box-shadow:0 2px 12px rgba(0,0,0,.4);background:#fff;max-width:100%;}}
#pdf-page-num{{min-width:50px;text-align:center;color:#fff;}}
.katex-formula{{overflow-x:auto;overflow-y:hidden;padding:8px 0;text-align:center;}}
.mermaid{{overflow-x:auto;text-align:center;margin:15px 0;padding:10px;border-radius:6px;background:#f8f8f8;}}
/* Modal */
.modal-overlay{{display:none;position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,.5);z-index:2000;justify-content:center;align-items:center;}}
.modal-overlay.show{{display:flex;}}
.modal{{background:#1a1a2e;border-radius:8px;padding:24px;min-width:360px;box-shadow:0 8px 32px rgba(0,0,0,.5);}}
.modal h2{{color:#e94560;margin-bottom:16px;font-size:16px;}}
.modal label{{color:#a8b2d1;font-size:13px;display:block;margin:12px 0 4px;}}
.modal input,.modal select{{width:100%;padding:8px 12px;background:#2c2c3e;border:1px solid #333;color:#cdd6f4;border-radius:4px;font-size:14px;outline:none;}}
.modal input:focus,.modal select:focus{{border-color:#e94560;}}
.modal .btn-row{{display:flex;gap:8px;justify-content:flex-end;margin-top:20px;}}
.modal .btn-row button{{padding:8px 20px;border:none;border-radius:4px;cursor:pointer;font-size:13px;}}
.btn-primary{{background:#e94560;color:#fff;}}
.btn-primary:hover{{background:#c23150;}}
.btn-secondary{{background:#333;color:#a8b2d1;}}
.btn-secondary:hover{{background:#444;}}
/* Toast */
#toast{{position:fixed;bottom:24px;right:24px;background:#1a1a2e;color:#fff;padding:10px 20px;border-radius:6px;font-size:13px;z-index:3000;opacity:0;transition:opacity .3s;box-shadow:0 4px 12px rgba(0,0,0,.3);pointer-events:none;}}
#toast.show{{opacity:1;}}
#toast.error{{background:#c0392b;}}
@media print{{*{{background:transparent!important;box-shadow:none!important;text-shadow:none!important;}}body{{overflow:visible!important;height:auto!important;display:block!important;}}#workspace{{overflow:visible!important;display:block!important;height:auto!important;}}#menubar,#sidebar,#editor-panel,.resizer,#toast{{display:none!important;}}#preview{{margin:0;padding:20px;overflow:visible!important;width:100%;height:auto!important;flex:none!important;display:block!important;}}}}
/* Code with line numbers */
.code-with-lines{{display:flex;background:#1a1a2e;border-radius:8px;overflow:hidden;margin:15px 0;}}
.code-with-lines pre{{margin:0;padding:16px 0;background:transparent;}}
.code-with-lines .line-numbers{{min-width:40px;text-align:right;padding-right:12px;padding-left:12px;color:#555;border-right:1px solid #333;user-select:none;font:13px/1.55 'Consolas','Courier New',monospace;overflow:hidden;flex-shrink:0;}}
.code-with-lines .line-numbers code{{color:#555!important;background:transparent!important;padding:0!important;}}
.code-with-lines pre:last-child{{flex:1;padding-left:16px;overflow-x:auto;}}
.code-with-lines pre:last-child code{{background:transparent!important;padding:0!important;}}
#preview .code-with-lines pre code{{background:transparent;padding:0;}}
</style>
</head><body>
<div id="menubar">
  <div class="file-menu">
    <button id="file-btn" onclick="toggleFileMenu(event)">File ▾</button>
    <div class="dropdown" id="file-dropdown">
      <a onclick="newFileDialog()">📄 New...</a>
      <a onclick="saveFile()" id="save-menuitem">💾 Save</a>
      <div class="divider"></div>
      <a onclick="exportPdf()">📄 Export PDF</a>
    </div>
  </div>
  <div class="file-menu">
    <button id="edit-btn" onclick="toggleEditMenu(event)">Edit ▾</button>
    <div class="dropdown" id="edit-dropdown">
      <a onclick="showFind()">🔍 Find <small>(Ctrl+F)</small></a>
      <a onclick="showReplace()">🔄 Replace <small>(Ctrl+H)</small></a>
      <div class="divider"></div>
      <a onclick="editor.focus();document.execCommand('undo')">↩ Undo <small>(Ctrl+Z)</small></a>
      <a onclick="editor.focus();document.execCommand('redo')">↪ Redo <small>(Ctrl+Y)</small></a>
    </div>
  </div>
  <span id="file-info"></span>
  <span id="save-indicator">●</span>
</div>
<div id="workspace">
<nav id="sidebar">{nav}</nav>
<div class="resizer" id="resizer1"></div>
<div id="editor-panel">
  <div id="editor-header">
    <span id="editor-filename">No file open</span>
    <span id="editor-lang"></span>
  </div>
  <textarea id="editor" placeholder="Select a file from the sidebar to edit, or File → New to create one." spellcheck="false"></textarea>
</div>
<div class="resizer" id="resizer2"></div>
<main id="preview" data-file="{file}" data-dir="{dir}" data-pdf="{pdf_file}">{content}</main>
<script id="raw-content" type="application/json">{raw_content}</script>
</div>
<div class="modal-overlay" id="new-file-modal">
  <div class="modal">
    <h2>New File</h2>
    <label for="new-file-type">File Type</label>
    <select id="new-file-type"></select>
    <label for="new-file-name">Filename (without extension)</label>
    <input type="text" id="new-file-name" placeholder="e.g. readme">
    <div class="btn-row">
      <button class="btn-secondary" onclick="closeNewFileDialog()">Cancel</button>
      <button class="btn-primary" onclick="createNewFile()">Create</button>
    </div>
  </div>
</div>
<div id="toast"></div>
<div id="find-bar" style="display:none;background:#2c2c3e;padding:6px 12px;display:none;align-items:center;gap:8px;flex-shrink:0;">
  <input id="find-input" type="text" placeholder="Find..." style="background:#1e1e2e;border:1px solid #444;color:#cdd6f4;padding:3px 8px;border-radius:3px;font:13px monospace;width:180px;" oninput="doFind()" onkeydown="findKey(event)">
  <span id="find-count" style="color:#888;font-size:12px;min-width:40px;"></span>
  <button onclick="doFind(-1)" style="background:#333;color:#a8b2d1;border:none;padding:3px 10px;border-radius:3px;cursor:pointer;font-size:12px;" title="Previous">◀</button>
  <button onclick="doFind(1)" style="background:#333;color:#a8b2d1;border:none;padding:3px 10px;border-radius:3px;cursor:pointer;font-size:12px;" title="Next">▶</button>
  <input id="replace-input" type="text" placeholder="Replace..." style="background:#1e1e2e;border:1px solid #555;color:#cdd6f4;padding:3px 8px;border-radius:3px;font:13px monospace;width:160px;display:none;">
  <button id="replace-btn" onclick="doReplace()" style="display:none;background:#e94560;color:#fff;border:none;padding:3px 10px;border-radius:3px;cursor:pointer;font-size:12px;">Replace</button>
  <button id="replace-all-btn" onclick="doReplaceAll()" style="display:none;background:#e94560;color:#fff;border:none;padding:3px 10px;border-radius:3px;cursor:pointer;font-size:12px;">All</button>
  <button onclick="closeFind()" style="background:none;border:none;color:#a8b2d1;cursor:pointer;font-size:16px;margin-left:auto;" title="Close (Escape)">✕</button>
</div>
<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/katex.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/katex@0.16.11/dist/contrib/auto-render.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/mermaid@10.9.0/dist/mermaid.min.js"></script>
<script src="https://cdnjs.cloudflare.com/ajax/libs/html2pdf.js/0.10.1/html2pdf.bundle.min.js"></script>
<script src="https://cdn.jsdelivr.net/npm/marked/marked.min.js"></script>
<script>
(function(){{
var HLJS_LANG={{'rs':'rust','py':'python','lock':'toml','yml':'yaml','js':'javascript','ts':'typescript','sh':'bash','ps1':'powershell','log':'plaintext','c':'c','h':'c','toml':'toml','json':'json','yaml':'yaml','ini':'ini','cfg':'ini','txt':'plaintext','ipynb':'json','tex':'latex'}};
var SUPPORTED_EXTS=['md','rs','py','toml','lock','json','yaml','yml','txt','ini','cfg','log','c','h','ipynb','tex','js','css','png','jpg','jpeg','gif','svg','pdf'];
var EXT_LABELS={{'md':'Markdown','rs':'Rust','py':'Python','toml':'TOML','lock':'TOML','json':'JSON','yaml':'YAML','yml':'YAML','txt':'Plain Text','ini':'INI','cfg':'INI','log':'Log','c':'C','h':'C Header','ipynb':'Jupyter Notebook','tex':'LaTeX','js':'JavaScript','css':'CSS','png':'PNG Image','jpg':'JPEG Image','jpeg':'JPEG Image','gif':'GIF Image','svg':'SVG Image','pdf':'PDF'}};

var currentFile=null,currentExt=null,isModified=false;
var sidebar=document.getElementById('sidebar');
var editor=document.getElementById('editor');
var preview=document.getElementById('preview');
var katexOpts={{delimiters:[{{left:'$$',right:'$$',display:true}},{{left:'$',right:'$',display:false}}],ignoredTags:['script','noscript','style','textarea','option']}};

// === Init: load file from embedded raw content ===
var rawEl=document.getElementById('raw-content');
if(rawEl&&rawEl.textContent){{
  try{{
    var data=JSON.parse(rawEl.textContent);
    if(data.file){{
      editor.value=data.content;
      currentFile={{dir:data.dir||'',filename:data.file}};
      currentExt=data.file.split('.').pop().toLowerCase();
      document.getElementById('editor-filename').textContent=data.file;
      document.getElementById('editor-lang').textContent=EXT_LABELS[currentExt]||currentExt;
      document.getElementById('file-info').textContent=(data.dir?data.dir+'/':'')+data.file;
      isModified=false;
      updateSaveIndicator();
      // Server-rendered preview is better (handles LaTeX/protect_latex perfectly).
      // Only call updatePreview for non-md files where code highlighting is needed.
      if(currentExt!=='ipynb'&&currentExt!=='md') updatePreview();
      sidebar.querySelectorAll('a').forEach(function(a){{a.classList.remove('active');}});
      sidebar.querySelectorAll('a').forEach(function(a){{
        if(a.textContent.trim()===data.file) a.classList.add('active');
      }});
      console.log('[MD Viewer] Loaded from embed:',data.file,data.content.length,'bytes');
    }}
  }}catch(e){{console.log('[MD Viewer] Embed parse failed, trying fetch fallback:',e.message);
    var initFile=preview.getAttribute('data-file');
    var initDir=preview.getAttribute('data-dir');
    if(initFile) loadFile(initFile,initDir||'');
  }}
}}else{{
  // Fallback to fetch
  var initFile=preview.getAttribute('data-file');
  var initDir=preview.getAttribute('data-dir');
  if(initFile){{loadFile(initFile,initDir||'');}}
}}

// === File Menu ===
window.toggleFileMenu=function(e){{e.stopPropagation();document.getElementById('file-dropdown').classList.toggle('show');document.getElementById('edit-dropdown').classList.remove('show');}};
document.addEventListener('click',function(){{document.getElementById('file-dropdown').classList.remove('show');document.getElementById('edit-dropdown').classList.remove('show');}});

// === Edit Menu ===
window.toggleEditMenu=function(e){{e.stopPropagation();document.getElementById('edit-dropdown').classList.toggle('show');document.getElementById('file-dropdown').classList.remove('show');}};
var findIdx=0,findText='';
function showFind(){{var bar=document.getElementById('find-bar');bar.style.display='flex';document.getElementById('find-input').focus();document.getElementById('replace-input').style.display='none';document.getElementById('replace-btn').style.display='none';document.getElementById('replace-all-btn').style.display='none';findText='';document.getElementById('find-input').value='';doFind();}}
function showReplace(){{var bar=document.getElementById('find-bar');bar.style.display='flex';document.getElementById('find-input').focus();document.getElementById('replace-input').style.display='inline';document.getElementById('replace-btn').style.display='inline';document.getElementById('replace-all-btn').style.display='inline';findText='';document.getElementById('find-input').value='';doFind();}}
function doFind(dir){{var sel=editor.value;var q=document.getElementById('find-input').value;if(q!==findText){{findIdx=0;findText=q;}};if(!q){{document.getElementById('find-count').textContent='';return;}};var re=new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g,'\\$&'),'gi');var cnt=(sel.match(re)||[]).length;document.getElementById('find-count').textContent=cnt?((findIdx%cnt)+1)+'/'+cnt:'0';if(!cnt)return;if(!dir)dir=1;if(dir>0){{var pos=sel.indexOf(q,findIdx);if(pos===-1){{findIdx=0;pos=sel.indexOf(q,0);}};if(pos!==-1){{editor.setSelectionRange(pos,pos+q.length);editor.focus();findIdx=pos+q.length;}}}}else{{var last=-1,pos2=0;while(pos2!==-1){{pos2=sel.indexOf(q,pos2);if(pos2<findIdx&&pos2!==-1){{last=pos2;}};if(pos2!==-1)pos2+=q.length;}};if(last===-1){{last=sel.lastIndexOf(q);}};if(last!==-1){{editor.setSelectionRange(last,last+q.length);editor.focus();findIdx=last;}}}}}}
function doReplace(){{var q=document.getElementById('find-input').value;var r=document.getElementById('replace-input').value;if(!q)return;var s=editor.selectionStart;var v=editor.value;if(v.substring(s,s+q.length)===q){{editor.value=v.substring(0,s)+r+v.substring(s+q.length);editor.setSelectionRange(s,s+r.length);}};doFind(1);}}
function doReplaceAll(){{var q=document.getElementById('find-input').value;var r=document.getElementById('replace-input').value;if(!q)return;var re=new RegExp(q.replace(/[.*+?^${}()|[\]\\]/g,'\\$&'),'g');var cnt=(editor.value.match(re)||[]).length;editor.value=editor.value.replace(re,r);toast('Replaced '+cnt+' occurrences');}}
function findKey(e){{if(e.key==='Enter'){{e.preventDefault();e.shiftKey?doFind(-1):doFind(1);}}}}
function closeFind(){{document.getElementById('find-bar').style.display='none';editor.focus();}}
// Expose to global scope for HTML onclick handlers
window.doFind=doFind;window.doReplace=doReplace;window.doReplaceAll=doReplaceAll;window.findKey=findKey;window.closeFind=closeFind;

// === Load file (called from init and save) ===
var IMG_EXTS=['png','jpg','jpeg','gif','svg'];
function loadFile(filename,dir){{
  var ext=filename.split('.').pop().toLowerCase();
  // 图片文件: 直接显示, 不通过api/raw加载文本
  if(IMG_EXTS.indexOf(ext)>=0){{
    currentFile={{dir:dir||'',filename:filename}};
    currentExt=ext;
    document.getElementById('editor-filename').textContent=filename;
    document.getElementById('editor-lang').textContent=EXT_LABELS[ext]||ext;
    document.getElementById('file-info').textContent=(dir?dir+'/':'')+filename;
    editor.value='';
    isModified=false;
    updateSaveIndicator();
    var imgPath=dir?dir+'/'+filename:filename;
    preview.innerHTML='<h1>'+escHtml(filename)+'</h1><img src="'+escHtml(imgPath)+'" style="max-width:100%;border-radius:8px;box-shadow:0 2px 12px rgba(0,0,0,.15);">';
    // Highlight active in sidebar
    sidebar.querySelectorAll('a').forEach(function(a){{a.classList.remove('active');}});
    sidebar.querySelectorAll('a').forEach(function(a){{
      if(a.textContent.trim()===filename) a.classList.add('active');
    }});
    var newUrl='/'+encodeURIComponent(filename)+(dir?'?dir='+encodeURIComponent(dir):'');
    if(location.pathname+location.search!==newUrl) history.replaceState(null,'',newUrl);
    return;
  }}
  // PDF: use server-side HTML rendering instead of raw fetch
  if(ext==='pdf'){{
    var pdfUrl='/'+encodeURIComponent(filename)+(dir?'?dir='+encodeURIComponent(dir):'');
    fetch(pdfUrl).then(function(r){{if(!r.ok)throw new Error('Not found');return r.text();}}).then(function(html){{
      // Extract the pdf2html content from the full page HTML
      var match=html.match(/<div class=\'pdf2html[\s\S]*?<\/div>\s*<\/div>/);
      if(match){{
        var pdfHtml='<div class=\'pdf2html-container\'>'+match[0]+'</div>';
        editor.value=pdfHtml;
        currentFile={{dir:dir||'',filename:filename}};currentExt='pdf';
        document.getElementById('editor-filename').textContent=filename;
        document.getElementById('editor-lang').textContent='PDF';
        preview.innerHTML=pdfHtml;
        updateSaveIndicator();
      }}else{{
        editor.value='[PDF渲染错误: 未找到内容]';
      }}
      isModified=false;
    }}).catch(function(e){{editor.value='[PDF加载失败: '+e.message+']';}});
    return;
  }}
  var qs=dir?'&dir='+encodeURIComponent(dir):'';
  var furl='/api/raw?file='+encodeURIComponent(filename)+qs;
  fetch(furl).then(function(r){{if(!r.ok)throw new Error('Not found');return r.text();}}).then(function(text){{
    editor.value=text;
    currentFile={{dir:dir||'',filename:filename}};
    currentExt=filename.split('.').pop().toLowerCase();
    document.getElementById('editor-filename').textContent=filename;
    document.getElementById('editor-lang').textContent=EXT_LABELS[currentExt]||currentExt;
    document.getElementById('file-info').textContent=(dir?dir+'/':'')+filename;
    isModified=false;
    updateSaveIndicator();
    updatePreview();
    // Highlight active in sidebar
    sidebar.querySelectorAll('a').forEach(function(a){{a.classList.remove('active');}});
    sidebar.querySelectorAll('a').forEach(function(a){{
      if(a.textContent.trim()===filename) a.classList.add('active');
    }});
    // Update URL without reload
    var newUrl='/'+encodeURIComponent(filename)+(dir?'?dir='+encodeURIComponent(dir):'');
    if(location.pathname+location.search!==newUrl) history.replaceState(null,'',newUrl);
  }}).catch(function(err){{toast('Failed to load: '+err.message,true);}});
}}

// === Save ===
window.saveFile=function(){{
  if(!currentFile){{toast('No file open. Select or create a file first.',true);return;}}
  var content=editor.value;
  var body=JSON.stringify({{dir:currentFile.dir,filename:currentFile.filename,content:content}});
  var xhr=new XMLHttpRequest();
  xhr.open('POST','api/save',true);
  xhr.setRequestHeader('Content-Type','application/json;charset=UTF-8');
  xhr.onload=function(){{
    if(xhr.status===200){{
      try{{
        var data=JSON.parse(xhr.responseText);
        if(data.ok){{
          isModified=false;
          updateSaveIndicator();
          // Use server-side render for perfect LaTeX
          var rbody=JSON.stringify({{content:content,ext:currentExt}});
          var rxhr=new XMLHttpRequest();
          rxhr.open('POST','api/render',true);
          rxhr.setRequestHeader('Content-Type','application/json;charset=UTF-8');
          rxhr.onload=function(){{
            if(rxhr.status===200){{
              preview.innerHTML=rxhr.responseText;
              renderKatexAndHL();
            }}
          }};
          rxhr.send(rbody);
          toast('Saved: '+currentFile.filename);
        }}else{{toast('Save failed: '+(data.error||'unknown'),true);}}
      }}catch(e){{toast('Save failed: invalid response - '+e.message,true);}}
    }}else{{toast('Save failed: HTTP '+xhr.status,true);}}
  }};
  xhr.onerror=function(){{toast('Save failed: network error',true);}};
  xhr.send(body);
}};

// === New File Dialog ===
window.newFileDialog=function(){{
  var sel=document.getElementById('new-file-type');
  sel.innerHTML='';
  SUPPORTED_EXTS.forEach(function(ext){{
    var opt=document.createElement('option');
    opt.value=ext;opt.textContent=EXT_LABELS[ext]+' (.'+ext+')';
    sel.appendChild(opt);
  }});
  document.getElementById('new-file-name').value='';
  document.getElementById('new-file-modal').classList.add('show');
  setTimeout(function(){{document.getElementById('new-file-name').focus();}},100);
}};
window.closeNewFileDialog=function(){{document.getElementById('new-file-modal').classList.remove('show');}};
window.createNewFile=function(){{
  var ext=document.getElementById('new-file-type').value;
  var name=document.getElementById('new-file-name').value.trim();
  if(!name){{toast('Please enter a filename.',true);return;}}
  var dir=currentFile?currentFile.dir:new URLSearchParams(location.search).get('dir')||'';
  var body=JSON.stringify({{dir:dir,filename:name,ext:ext}});
  var xhr=new XMLHttpRequest();
  xhr.open('POST','api/new',true);
  xhr.setRequestHeader('Content-Type','application/json;charset=UTF-8');
  xhr.onload=function(){{
    if(xhr.status===200){{
      try{{
        var data=JSON.parse(xhr.responseText);
        if(data.ok){{
          closeNewFileDialog();
          var fname=name+'.'+ext;
          toast('Created: '+fname);
          setTimeout(function(){{location.reload();}},300);
        }}else{{toast('Failed: '+(data.error||'unknown'),true);}}
      }}catch(e){{toast('Failed: '+e.message,true);}}
    }}else{{toast('Failed: HTTP '+xhr.status,true);}}
  }};
  xhr.onerror=function(){{toast('Failed: network error',true);}};
  xhr.send(body);
}};

// === Preview update ===
function updatePreview(){{
  var text=editor.value||'';
  // PDF viewer is server-rendered, don't overwrite
  if(currentExt==='pdf'){{if(editor.value){preview.innerHTML=editor.value;}return;}}
  if(currentExt==='md'){{
    try{{
      // Protect LaTeX math from marked's markdown parsing
      var math=[];
      text=text.replace(/\$\$([\s\S]*?)\$\$/g,function(m,c){{math.push({{d:1,v:c}});return '␜M'+(math.length-1)+'␜';}});
      text=text.replace(/\$([^\$\n]+?)\$/g,function(m,c){{math.push({{d:0,v:c}});return '␜I'+(math.length-1)+'␜';}});
      var html=marked.parse(text);
      html=html.replace(/␜M(\d+)␜/g,function(m,i){{var b=math[parseInt(i)];return '$$'+b.v+'$$';}});
      html=html.replace(/␜I(\d+)␜/g,function(m,i){{var b=math[parseInt(i)];return '$'+b.v+'$';}});
preview.innerHTML=html;
      // Render mermaid blocks
      var mb=preview.querySelectorAll('pre code.language-mermaid');
      if(mb.length>0&&typeof mermaid!=='undefined'){{
        mb.forEach(function(c){{var d=document.createElement('div');d.className='mermaid';d.textContent=c.textContent;c.parentElement.replaceWith(d);}});
        mermaid.initialize({{startOnLoad:false,theme:'default'}});
        mermaid.run({{querySelector:'.mermaid'}}).then(function(){{renderKatexAndHL();}}).catch(function(){{renderKatexAndHL();}});
      }}else{{renderKatexAndHL();}}
    }}catch(e){{preview.innerHTML='<p style="color:red">Markdown render error: '+escHtml(e.message)+'</p>';}}
  }}else if(currentExt==='tex'){{
    var rawHtml='<h1>'+escHtml(currentFile?currentFile.filename:'')+'</h1>'+texToHtml(text);
    preview.innerHTML=renderHtml(rawHtml);
  }}else if(['png','jpg','jpeg','gif','svg'].indexOf(currentExt)>=0){{
    // 图片文件: 显示图像预览
    var imgPath=currentFile.dir?currentFile.dir+'/'+currentFile.filename:currentFile.filename;
    preview.innerHTML='<h1>'+escHtml(currentFile.filename)+'</h1><img src="'+escHtml(imgPath)+'" style="max-width:100%;border-radius:8px;box-shadow:0 2px 12px rgba(0,0,0,.15);">';
  }}else{{
    var lang=HLJS_LANG[currentExt]||currentExt;
    var lines=text.split('\n');
    var lineNums=lines.map(function(_,i){return i+1;}).join('\n');
    preview.innerHTML='<h1>'+escHtml(currentFile?currentFile.filename:'')+'</h1><div class="code-with-lines"><pre class="line-numbers">'+lineNums+'</pre><pre><code class="language-'+lang+'">'+escHtml(text)+'</code></pre></div>';
    hljs.highlightAll();
  }}
}}
function renderHtml(mathHtml){{
  // Render KaTeX on raw HTML string (before innerHTML to avoid & escaping)
  try{{
    // $$...$$ display math (must be done first)
    mathHtml=mathHtml.replace(/\$\$([\s\S]*?)\$\$/g,function(_,c){{
      try{{return katex.renderToString(c,{{displayMode:true,throwOnError:false}});}}catch(e){{return '<span class=\\"katex-error\\">[KaTeX: '+c.substring(0,40)+']</span>';}}
    }});
    // $...$ inline math
    mathHtml=mathHtml.replace(/\$([^\$\n]+?)\$/g,function(_,c){{
      try{{return katex.renderToString(c,{{displayMode:false,throwOnError:false}});}}catch(e){{return '<span class=\\"katex-error\\">$'+c+'$</span>';}}
    }});
  }}catch(e){{console.error('KaTeX render error:',e);}}
  return mathHtml;
}}
function renderKatexAndHL(){{
  try{{renderMathInElement(preview,katexOpts);}}catch(e){{}}
  hljs.highlightAll();
  fixImgPaths();
}}
// Fix relative image paths: prepend ?dir= from preview's data-dir attribute
function fixImgPaths(){{
  var d=preview.getAttribute('data-dir');
  if(d){{
    preview.querySelectorAll('img').forEach(function(img){{
      var src=img.getAttribute('src');
      if(src&&!src.includes('?dir=')&&!src.startsWith('http')&&!src.startsWith('data:')){{
        img.src=src+'?dir='+encodeURIComponent(d);
      }}
    }});
  }}
}}
function escHtml(s){{return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');}}

// === Export PDF ===
window.exportPdf=function(){{
  if(typeof html2pdf==='undefined'){{toast('PDF export library not loaded.',true);return;}}
  var oldOverflow=preview.style.overflow;var oldHeight=preview.style.height;var oldMaxH=preview.style.maxHeight;
  preview.style.overflow='visible';preview.style.height='auto';preview.style.maxHeight='none';
  var fn=(currentFile?currentFile.filename.replace(/\.[^.]+$/,''):'document')+'.pdf';
  html2pdf().set({{margin:[10,10,10,10],filename:fn,image:{{type:'jpeg',quality:.92}},html2canvas:{{scale:1.5,useCORS:true,windowHeight:800}},jsPDF:{{unit:'mm',format:'a4',orientation:'portrait'}}}}).from(preview).save().then(function(){{
    preview.style.overflow=oldOverflow;preview.style.height=oldHeight;preview.style.maxHeight=oldMaxH;
  }});
  toast('PDF exporting: '+fn);
}};

// === Toast ===
var toastTimer;
function toast(msg,isErr){{
  var t=document.getElementById('toast');
  t.textContent=msg;t.className=isErr?'error show':'show';
  clearTimeout(toastTimer);
  toastTimer=setTimeout(function(){{t.classList.remove('show','error');}},2500);
}}

// === Modified indicator ===
editor.addEventListener('input',function(){{
  if(currentFile&&!isModified){{isModified=true;updateSaveIndicator();}}
}});
function updateSaveIndicator(){{
  document.getElementById('save-indicator').style.display=isModified?'inline':'none';
}}

// === Keyboard shortcuts ===
document.addEventListener('keydown',function(e){{
  if((e.ctrlKey||e.metaKey)&&e.key==='s'){{e.preventDefault();saveFile();}}
  if((e.ctrlKey||e.metaKey)&&e.key==='f'){{e.preventDefault();showFind();}}
  if((e.ctrlKey||e.metaKey)&&e.key==='h'){{e.preventDefault();showReplace();}}
  if(e.key==='Escape'){{closeFind();document.getElementById('file-dropdown').classList.remove('show');document.getElementById('edit-dropdown').classList.remove('show');closeNewFileDialog();}}
}});
// Tab/Shift+Tab in editor
editor.addEventListener('keydown',function(e){{
  if(e.key!=='Tab')return;
  e.preventDefault();
  var ta=this,start=ta.selectionStart,end=ta.selectionEnd;
  var val=ta.value;
  if(e.shiftKey){{
    // Shift+Tab: unindent selected lines
    var lineStart=val.lastIndexOf('\n',start-1)+1;
    var lineEnd=val.indexOf('\n',end-1);
    if(lineEnd===-1)lineEnd=val.length;
    var before=val.substring(0,lineStart);
    var block=val.substring(lineStart,lineEnd);
    var after=val.substring(lineEnd);
    var lines=block.split('\n');
    var removed=0;
    for(var i=0;i<lines.length;i++){{
      if(lines[i].charAt(0)==='\t'){{lines[i]=lines[i].substring(1);removed++;}}
      else if(lines[i].startsWith('    ')){{lines[i]=lines[i].substring(4);removed+=4;}}
    }}
    ta.value=before+lines.join('\n')+after;
    ta.selectionStart=Math.max(lineStart,start-(removed>0?1:0));
    ta.selectionEnd=end-removed;
  }}else{{
    // Tab: indent selected lines or insert tab
    if(start===end){{
      // No selection: insert tab at cursor
      ta.value=val.substring(0,start)+'\t'+val.substring(end);
      ta.selectionStart=ta.selectionEnd=start+1;
    }}else{{
      // Selection: indent each line
      var ls=val.lastIndexOf('\n',start-1)+1;
      var le=val.indexOf('\n',end-1);
      if(le===-1)le=val.length;
      var bef=val.substring(0,ls);
      var blk=val.substring(ls,le);
      var aft=val.substring(le);
      ta.value=bef+'\t'+blk.replace(/\n/g,'\n\t')+aft;
      ta.selectionStart=ls;
      ta.selectionEnd=ls+1+blk.length+(blk.split('\n').length-1);
    }}
    ta.dispatchEvent(new Event('input',{{bubbles:true}}));
  }}
}});

// === Resizers ===
function setupResizer(resizerId,leftSel,rightSel,storageKey,minL,minR,defaultPctL){{
  var resizer=document.getElementById(resizerId);
  var left=document.querySelector(leftSel);
  var right=document.querySelector(rightSel);
  var isDragging=false;
  var startX,startW;
  // restore
  var saved=localStorage.getItem(storageKey);
  if(saved)left.style.width=saved+'px';
  resizer.addEventListener('mousedown',function(e){{isDragging=true;resizer.classList.add('dragging');startX=e.clientX;startW=left.getBoundingClientRect().width;e.preventDefault();}});
  document.addEventListener('mousemove',function(e){{
    if(!isDragging)return;
    var w=startW+(e.clientX-startX);
    w=Math.max(minL,w);
    // Don't let it push right panel too small
    var workspace=document.getElementById('workspace');
    var tw=workspace.getBoundingClientRect().width;
    var rw=tw-w-resizer.getBoundingClientRect().width;
    if(rw<minR)w=tw-resizer.getBoundingClientRect().width-minR;
    left.style.width=w+'px';
  }});
  document.addEventListener('mouseup',function(){{
    if(isDragging){{isDragging=false;resizer.classList.remove('dragging');localStorage.setItem(storageKey,parseInt(left.style.width));}}
  }});
}}
setupResizer('resizer1','#sidebar','#editor-panel','md_sidebar_w',80,150);
setupResizer('resizer2','#editor-panel','#preview','md_editor_w',150,150);

// === Modal overlay click ===
document.getElementById('new-file-modal').addEventListener('click',function(e){{if(e.target===this)closeNewFileDialog();}});

// === Close dropdown on Escape ===
document.addEventListener('keydown',function(e){{if(e.key==='Escape'){{document.getElementById('file-dropdown').classList.remove('show');closeNewFileDialog();}}}});

console.log('[MD Viewer] Editor mode ready. supported:',SUPPORTED_EXTS.join(', '));
// Ensure LaTeX/mermaid/highlight on initial server-rendered content
setTimeout(function(){{renderKatexAndHL();}},100);

// === TeX to HTML converter ===
function texToHtml(src){{
  console.log('texToHtml called, len:',src.length,'has align:',/\\begin\{align\*?\}/.test(src),'has begin_doc:',/\\begin\{document\}/.test(src));
  console.log('First 150:',src.substring(0,150).replace(/\n/g,'\\n'));
  console.log('Searching for backslash-begin:',src.indexOf('\\begin')>=0,'at pos:',src.indexOf('\\begin'));
  var m=[], i=0;
  // 1) Protect ALL math environments FIRST (before any text conversion)
  var mathPatterns=[
    {re:/\\begin\{align\*?\}([\s\S]*?)\\end\{align\*?\}/g, p:'A'},
    {re:/\\begin\{aligned\}([\s\S]*?)\\end\{aligned\}/g, p:'L'},
    {re:/\\begin\{equation\}([\s\S]*?)\\end\{equation\}/g, p:'E'},
    {re:/\\begin\{eqnarray\}([\s\S]*?)\\end\{eqnarray\}/g, p:'Q'},
    {re:/\\\[([\s\S]*?)\\\]/g, p:'D'},
    {re:/\$\$([\s\S]*?)\$\$/g, p:'M'},
    {re:/\$([^\$\n]+?)\$/g, p:'I'},
  ];
  for(var pi=0;pi<mathPatterns.length;pi++){{
    var before = src.length;
    var matchCount = 0;
    src=src.replace(mathPatterns[pi].re,function(c){
      matchCount++;
      console.log('MATCH pattern',mathPatterns[pi].p,'found at',c.substring(0,30),'len='+c.length);
      if(mathPatterns[pi].p === 'I') {{
        // DON'T wrap $...$ in $$ - let KaTeX handle $...$ natively
        m.push(c);
        return '␜I'+(i++)+'␜';
      }}
      // For display math: wrap in $$ so KaTeX finds it
      var cleaned = c;
      if(mathPatterns[pi].p === 'M') cleaned = c.slice(2, -2); // remove original $$...$$
      // KaTeX requires aligned inside $$, not align
      if(mathPatterns[pi].p === 'A') cleaned = cleaned.replace(/\\begin\{align\*?\}/g,'\\begin{aligned}').replace(/\\end\{align\*?\}/g,'\\end{aligned}');
      m.push('$$'+cleaned+'$$');
      return '␜'+mathPatterns[pi].p+(i++)+'␜';
    });
  }}
  
  // 2) Strip preamble and comments
  src=src.replace(/^[\s\S]*?\\begin\{document\}/,'');
  src=src.replace(/\\end\{document\}[\s\S]*$/,'');
  src=src.replace(/^[ \t]*%.*$/gm,'');
  
  // 3) Title block
  var title='', author='', date='';
  src=src.replace(/\\title\{([^}]*)\}/g,function(_,t){{title=t;return '';}});
  src=src.replace(/\\author\{([^}]*)\}/g,function(_,a){{author=a.replace(/\\\\/g,', ');return '';}});
  src=src.replace(/\\date\{([^}]*)\}/g,function(_,d){{date=d;return '';}});
  src=src.replace(/\\maketitle/g,'');
  
  // 4) Section headings
  src=src.replace(/\\section\{([^}]*)\}/g,'<h2 class="tex-section">$1</h2>');
  src=src.replace(/\\subsection\{([^}]*)\}/g,'<h3 class="tex-subsection">$1</h3>');
  src=src.replace(/\\subsubsection\{([^}]*)\}/g,'<h4 class="tex-subsubsection">$1</h4>');
  src=src.replace(/\\paragraph\{([^}]*)\}/g,'<h5 class="tex-paragraph">$1</h5>');
  src=src.replace(/\\begin\{abstract\}([\s\S]*?)\\end\{abstract\}/g,'<div class="tex-abstract"><strong>Abstract.</strong> $1</div>');
  src=src.replace(/\\bibliographystyle\{[^}]*\}/g,'');
  // Bibliography
  src=src.replace(/\\begin\{thebibliography\}\{[^}]*\}([\s\S]*?)\\end\{thebibliography\}/g,function(_,b){{
    b=b.replace(/\\bibitem\{([^}]*)\}/g,'<p class="tex-ref-entry" id="ref-$1"><strong>[$1]</strong> ');
    b=b.replace(/\\newblock /g,'<br>');
    b=b.replace(/\\newblock/g,'<br>');
    return '<div class="tex-references"><h2 class="tex-section">References</h2>'+b+'</p></div>';
  }});
  
  // 5) Lists
  src=src.replace(/\\begin\{itemize\}([\s\S]*?)\\end\{itemize\}/g,function(_,b){{
    var items=b.replace(/\\item\b[^\n]*/g,function(it){{return '</li><li>'+it.replace(/\\item\b */,'');}});
    return '<ul>'+items.replace(/^<\/li>/,'').replace(/<li>\s*<\/li>/g,'')+'</li></ul>';
  }});
  src=src.replace(/\\begin\{enumerate\}([\s\S]*?)\\end\{enumerate\}/g,function(_,b){{
    var items=b.replace(/\\item\b[^\n]*/g,function(it){{return '</li><li>'+it.replace(/\\item\b */,'');}});
    return '<ol>'+items.replace(/^<\/li>/,'').replace(/<li>\s*<\/li>/g,'')+'</li></ol>';
  }});
  
  // 6) Inline formatting (only outside math)
  src=src.replace(/\\textbf\{([^}]*)\}/g,'<strong>$1</strong>');
  src=src.replace(/\\textit\{([^}]*)\}/g,'<em>$1</em>');
  src=src.replace(/\\texttt\{([^}]*)\}/g,'<code>$1</code>');
  src=src.replace(/\\emph\{([^}]*)\}/g,'<em>$1</em>');
  src=src.replace(/\\url\{([^}]*)\}/g,'<a href="$1" target="_blank">$1</a>');
  src=src.replace(/\\href\{([^}]*)\}\{([^}]*)\}/g,'<a href="$1" target="_blank">$2</a>');
  src=src.replace(/\\label\{([^}]*)\}/g,'<span id="$1"></span>');
  src=src.replace(/\\ref\{([^}]*)\}/g,'<a href="#$1">$1</a>');
  src=src.replace(/\\citep\{([^}]*)\}/g,'<span class="tex-cite">[$1]</span>');
  src=src.replace(/\\citet\{([^}]*)\}/g,'<span class="tex-cite">$1</span>');
  // DO NOT convert \text{} - it's a math command handled by KaTeX
  
  // 7) Tables
  src=src.replace(/\\begin\{tabular\}\{([^}]*)\}([\s\S]*?)\\end\{tabular\}/g,function(_,fmt,body){{
    var rows=body.trim().split('\\\\').map(function(r){{
      return '<tr>'+r.trim().split('&').map(function(c){{return '<td>'+c.trim()+'</td>';}}).join('')+'</tr>';
    }}).join('');
    return '<table class="tex-table">'+rows+'</table>';
  }});

  // 7b) Table/Figure/Caption/Includegraphics environments
  src=src.replace(/\\begin\{figure\}\[[^\]]*\]/g,'<div class="tex-figure">');
  src=src.replace(/\\end\{figure\}/g,'</div>');
  src=src.replace(/\\begin\{table\}\[[^\]]*\]/g,'<div class="tex-table-wrapper">');
  src=src.replace(/\\end\{table\}/g,'</div>');
  src=src.replace(/\\centering/g,'');
  src=src.replace(/\\caption\{([^}]*)\}/g,'<div class="tex-caption">$1</div>');
  src=src.replace(/\\label\{([^}]*)\}/g,'<a id="$1"></a>');
  src=src.replace(/\\toprule/g,'');
  src=src.replace(/\\midrule/g,'');
  src=src.replace(/\\bottomrule/g,'');
  src=src.replace(/\\includegraphics\[([^\]]*)\]\{([^}]*)\}/g,function(_,opts,path){{
    var w=''; var h='';
    opts.replace(/width=([\d.]+)/,function(_,v){{w=(parseFloat(v)*100)+'%';}});
    opts.replace(/height=([\d.]+)/,function(_,v){{h=(parseFloat(v)*100)+'%';}});
    return '<img src="'+path+'"'+(w?' style="width:'+w+'"' : '')+(h?' height="'+h+'"' : '')+' class="tex-figure-img">';
  }});
  src=src.replace(/\\includegraphics\{([^}]*)\}/g,'<img src="$1" class="tex-figure-img" style="max-width:100%">');

  // 8) Restore math environments
  src=src.replace(/␜([MIDEALQ])(\d+)␜/g,function(_,t,i){{return m[parseInt(i)]||'';}});
  
  // 9) Build title block (after restore so KaTeX can render math in title)
  if(title||author){{
    var h='<header class="tex-titlepage"><h1 class="tex-title">'+title+'</h1>';
    if(author) h+='<p class="tex-author">'+author+'</p>';
    if(date) h+='<p class="tex-date">'+date+'</p>';
    h+='</header>';
    src=h+src;
  }}
  
  // 10) Simple paragraph wrapping (skip lines inside $$...$$ display math)
  src=src.replace(/\n\n+/g,'\n');
  var lines=src.split('\n');
  var out=[];
  var inMath=false;
  for(var li=0;li<lines.length;li++){{
    var line=lines[li].trim();
    if(!line && !inMath) continue;
    if(/\$\$/.test(line)) inMath=!inMath;
    if(inMath||/^<(h[1-6]|div|table|ul|ol|li|header|p)/i.test(line)||/^<\/(div|ul|ol)/i.test(line)){{
      out.push(line);
    }}else{{
      out.push('<p class="tex-para">'+line+'</p>');
    }}
  }}
  return '<div class="tex-body">'+out.join('\n')+'</div>';
}}

}})();
</script>
</body></html>"##;

fn main() {
    let args: Vec<String> = env::args().collect();

    // parse port (first optional arg)
    let port: u16 = if args.len() > 1 {
        args[1].parse().unwrap_or(DEFAULT_PORT)
    } else {
        DEFAULT_PORT
    };

    // parse root dir (second optional arg)
    let root_dir: PathBuf = if args.len() > 2 {
        PathBuf::from(&args[2])
    } else {
        let cwd = env::current_dir().expect("Cannot get CWD");
        cwd.join("docs")
    };

    // parse base path (third optional arg, e.g. /docs)
    let base_path: String = if args.len() > 3 {
        let bp = args[3].trim_start_matches('/').to_string();
        if bp.is_empty() { String::new() } else { format!("/{}", bp) }
    } else {
        String::new()
    };

    let served = if root_dir.is_absolute() {
        root_dir.clone()
    } else {
        let cwd = env::current_dir().expect("Cannot get CWD");
        cwd.join(&root_dir)
    };

    let proj = served.canonicalize().unwrap_or(served.clone());

    if !proj.exists() {
        eprintln!("Error: root directory not found at {:?}", proj);
        std::process::exit(1);
    }

    let addr = format!("127.0.0.1:{}", port);
    let listener = TcpListener::bind(&addr).expect("Cannot bind");
    eprintln!("[files_browser] http://{}  serving {:?}", addr, proj);
    let p = proj.clone();
    let s = served.clone();
    for stream in listener.incoming() {
        if let Ok(st) = stream {
            let r = s.clone();
            let j = p.clone();
            let b = base_path.clone();
            thread::spawn(move || handle(st, &r, &j, &b));
        }
    }
}

struct Req {
    method: String, path: String, query: HashMap<String, String>, body: String,
}

fn parse(s: &mut TcpStream) -> Option<Req> {
    let mut raw = Vec::with_capacity(8192);
    let mut buf = [0u8; 8192];
    // Read headers (until \r\n\r\n)
    loop {
        let n = s.read(&mut buf).ok().filter(|&n| n > 0)?;
        raw.extend_from_slice(&buf[..n]);
        // Search for \r\n\r\n in raw
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if raw.len() > 65536 { return None; } // safety limit
    }
    let raw_str = String::from_utf8_lossy(&raw);
    let lines: Vec<&str> = raw_str.lines().collect();
    if lines.is_empty() { return None; }
    let parts: Vec<&str> = lines[0].split_whitespace().collect();
    if parts.len() < 2 { return None; }
    let method = parts[0].to_string();
    let path = parts[1].to_string();
    let mut q = HashMap::new();
    // Parse headers to find Content-Length
    let mut content_length: usize = 0;
    for line in &lines[1..] {
        if line.trim().is_empty() { break; }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("Content-Length") {
                content_length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    // Find body start (after \r\n\r\n)
    let body_start = raw.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4).unwrap_or(raw.len());
    let body_already = raw.len() - body_start;
    // Read remaining body bytes
    if content_length > body_already {
        let needed = content_length - body_already;
        raw.reserve(needed);
        let mut remain = vec![0u8; needed];
        let mut total = 0;
        while total < needed {
            if let Ok(n) = s.read(&mut remain[total..]) {
                if n == 0 { break; }
                total += n;
            } else { break; }
        }
        raw.extend_from_slice(&remain[..total]);
    }
    let body_bytes = if body_start < raw.len() { &raw[body_start..] } else { &[] };
    let body = String::from_utf8_lossy(body_bytes).to_string();

    // parse query
    let clean_path = if let Some((p, qs)) = path.split_once('?') {
        for pair in qs.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                q.insert(k.to_string(), url_decode(v).to_string());
            }
        }
        p.to_string()
    } else {
        path.clone()
    };
    Some(Req { method, path: clean_path, query: q, body })
}

fn handle(mut s: TcpStream, served: &Path, proj: &Path, base_path: &str) {
    let Some(req) = parse(&mut s) else { return };
    let path_raw = url_decode(&req.path.trim_start_matches('/')).to_string();
    // strip base_path prefix for routing (e.g. /docs/?dir=xxx → /?dir=xxx)
    let bp = base_path.trim_start_matches('/');
    let path = if !bp.is_empty() && path_raw.starts_with(bp) {
        path_raw[bp.len()..].to_string()
    } else {
        path_raw
    };
    let path = path.trim_start_matches('/').to_string();
    let q = req.query;
    let body = req.body;

    let method = req.method;
    eprintln!("[REQ] {} {} | q={:?} | body_len={}", method, path, q, body.len());

    // === API routes ===
    if path == "api/raw" {
        let file = q.get("file").cloned().unwrap_or_default();
        if file.is_empty() { send_404(&mut s); return; }
        let cr = cur_root(served, proj, &q);
        let fp = cr.join(&file);
        let text = if fp.is_file() {
            fs::read_to_string(&fp).ok()
        } else {
            walk(served, &file).and_then(|p| fs::read_to_string(&p).ok())
        };
        if let Some(t) = text {
            send(&mut s, 200, "text/plain;charset=utf-8", t.as_bytes());
        } else {
            send_404(&mut s);
        }
        return;
    }

    if path == "api/raw_bin" {
        let file = q.get("file").cloned().unwrap_or_default();
        if file.is_empty() { send_404(&mut s); return; }
        let cr = cur_root(served, proj, &q);
        let fp = cr.join(&file);
        let found = if fp.is_file() { Some(fp) } else { walk(served, &file) };
        if let Some(p) = found {
            match fs::read(&p) {
                Ok(bytes) => send(&mut s, 200, "application/pdf", &bytes),
                Err(_) => send_404(&mut s),
            }
        } else { send_404(&mut s); }
        return;
    }

    if path == "api/save" && method == "POST" {
        let body = body.trim().to_string();
        let dir = json_str(&body, "dir").unwrap_or_default();
        let filename = json_str(&body, "filename").unwrap_or_default();
        let content = json_str(&body, "content").unwrap_or_default();
        if filename.is_empty() {
            send(&mut s, 200, "application/json", b"{\"ok\":false,\"error\":\"filename required\"}");
            return;
        }
        let cr = if dir.is_empty() { served.to_path_buf() } else { served.join(&dir) };
        let fp = cr.join(&filename);
        if let Err(e) = fs::write(&fp, &content) {
            let resp = format!("{{\"ok\":false,\"error\":\"{}\"}}", e);
            send(&mut s, 200, "application/json", resp.as_bytes());
        } else {
            send(&mut s, 200, "application/json", b"{\"ok\":true}");
        }
        return;
    }

    if path == "api/render" && method == "POST" {
        let body = body.trim().to_string();
        let content = json_str(&body, "content").unwrap_or_default();
        let ext = json_str(&body, "ext").unwrap_or_default();
        let html = if ext == "md" {
            let mut opts = Options::empty();
            opts.insert(Options::ENABLE_TABLES);
            opts.insert(Options::ENABLE_FOOTNOTES);
            opts.insert(Options::ENABLE_STRIKETHROUGH);
            opts.insert(Options::ENABLE_TASKLISTS);
            let detabbed = strip_indent(&content);
            let (protected_text, latex_exprs) = protect_latex(&detabbed);
            let (protected_html, html_frags) = protect_html_fences(&protected_text);
            let parser = Parser::new_ext(&protected_html, opts);
            let mut html_buf = String::new();
            html::push_html(&mut html_buf, parser);
            let restored = restore_latex(&html_buf, &latex_exprs);
            restore_html(&restored, &html_frags)
        } else if ext == "ipynb" {
            render_ipynb(&content)
        } else if ext == "tex" {
            render_tex(&content)
        } else {
            let lang = hljs_lang(&ext);
            format!("<pre><code class=\"language-{}\">{}</code></pre>", lang, esc(&content))
        };
        send(&mut s, 200, "text/html;charset=utf-8", html.as_bytes());
        return;
    }

    if path == "api/new" && method == "POST" {
        let body = body.trim().to_string();
        let dir = json_str(&body, "dir").unwrap_or_default();
        let filename = json_str(&body, "filename").unwrap_or_default();
        let ext = json_str(&body, "ext").unwrap_or_default();
        if filename.is_empty() || ext.is_empty() {
            send(&mut s, 200, "application/json", b"{\"ok\":false,\"error\":\"filename and ext required\"}");
            return;
        }
        if !SUPPORTED_EXTS.contains(&ext.as_str()) {
            send(&mut s, 200, "application/json", b"{\"ok\":false,\"error\":\"unsupported extension\"}");
            return;
        }
        let cr = if dir.is_empty() { served.to_path_buf() } else { served.join(&dir) };
        let fname = format!("{}.{}", filename, ext);
        let fp = cr.join(&fname);
        if fp.exists() {
            send(&mut s, 200, "application/json", b"{\"ok\":false,\"error\":\"file already exists\"}");
            return;
        }
        if let Err(e) = fs::write(&fp, "") {
            let resp = format!("{{\"ok\":false,\"error\":\"{}\"}}", e);
            send(&mut s, 200, "application/json", resp.as_bytes());
        } else {
            send(&mut s, 200, "application/json", b"{\"ok\":true}");
        }
        return;
    }

    // route
    if path.is_empty() {
        let nav = nav_html(served, proj, &q, base_path);
        let cr = cur_root(served, proj, &q);
        let title = cr.file_name().and_then(|n| n.to_str()).unwrap_or("docs");
        let html = HTML_TPL
            .replace("{{", "{").replace("}}", "}")
            .replace("{title}", title)
            .replace("{nav}", &nav)
            .replace("{file}", "")
            .replace("{dir}", "")
            .replace("{pdf_file}", "")
            .replace("{content}", "")
            .replace("{raw_content}", "{}");
        send(&mut s, 200, "text/html;charset=utf-8", html.as_bytes());
    } else if is_supported(Path::new(&path)) && !is_image_ext(path.as_str()) {
        // try find and render
        let cr = cur_root(served, proj, &q);
        let fp = cr.join(&path);
        let is_pdf = path.ends_with(".pdf");
        let content = if is_pdf {
            // PDF is binary; just check existence, content will be loaded via api/raw_bin
            if fp.is_file() || walk(served, &path).is_some() {
                Some(String::new())
            } else {
                None
            }
        } else if fp.is_file() {
            fs::read_to_string(&fp).ok()
        } else {
            // recursive search
            fs::read_dir(served).ok().and_then(|_| {
                walk(served, &path).and_then(|p| fs::read_to_string(&p).ok())
            })
        };
        if let Some(text) = content {
            let nav = nav_html(served, proj, &q, base_path);
            let cr = cur_root(served, proj, &q);
            let dir_title = cr.file_name().and_then(|n| n.to_str()).unwrap_or("docs");
            let fname = fp.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            let ext = fp.extension().and_then(|x| x.to_str()).unwrap_or("");
            let dir_val = q.get("dir").cloned().unwrap_or_default();
            let content_html = if ext == "md" {
                let mut opts = Options::empty();
                opts.insert(Options::ENABLE_TABLES);
                opts.insert(Options::ENABLE_FOOTNOTES);
                opts.insert(Options::ENABLE_STRIKETHROUGH);
                opts.insert(Options::ENABLE_TASKLISTS);
                let detabbed = strip_indent(&text);
                let (protected_text, latex_exprs) = protect_latex(&detabbed);
                let (protected_html, html_frags) = protect_html_fences(&protected_text);
                let parser = Parser::new_ext(&protected_html, opts);
                let mut html_buf = String::new();
                html::push_html(&mut html_buf, parser);
                let restored = restore_latex(&html_buf, &latex_exprs);
                let restored_html = restore_html(&restored, &html_frags);
                format!("<article>{}</article>", restored_html)
            } else if ext == "ipynb" {
                render_ipynb(&text)
            } else if ext == "tex" {
                render_tex(&text)
            } else if ext == "pdf" {
                // 服务端PDF→HTML渲染（基于lopdf的内容流解释器）
                match pdf2html::handle_pdf2html(fp.to_str().unwrap_or("")) {
                    Ok(html) => format!(r#"<h1 style="margin-bottom:12px;color:#333;">{}</h1>{}"#, esc(fname), html),
                    Err(e) => format!(
                        r#"<div style="padding:24px;color:#c0392b;text-align:center;">
<h2>PDF 渲染失败</h2><p>{}</p>
<p style="margin-top:12px;font-size:12px;color:#888;">目前仅支持文本型PDF，扫描件/复杂字体PDF暂不可用</p>
</div>"#, esc(&e)),
                }
            } else {
                let lang = hljs_lang(ext);
                format!(
                    "<article><h1>{}</h1><pre><code class=\"language-{}\">{}</code></pre></article>",
                    esc(fname),
                    esc(lang),
                    esc(&text)
                )
            };
            // Rewrite relative image paths to include ?dir= for proper resolution
            let content_html = if !dir_val.is_empty() {
                let mut out = String::with_capacity(content_html.len() + 256);
                let mut rest = content_html.as_str();
                while let Some(p) = rest.find("src=\"") {
                    out.push_str(&rest[..p+5]);
                    rest = &rest[p+5..];
                    if let Some(cq) = rest.find('"') {
                        let sv = &rest[..cq];
                        if !sv.starts_with("http") && !sv.starts_with("data:") && !sv.contains('?') {
                            out.push_str(sv);
                            out.push_str("?dir=");
                            out.push_str(&dir_val);
                        } else {
                            out.push_str(sv);
                        }
                        out.push('"');
                        rest = &rest[cq+1..];
                    } else {
                        out.push_str(rest);
                        rest = "";
                    }
                }
                out.push_str(rest);
                out
            } else {
                content_html
            };
            let raw_json = format!(
                r#"{{"file":"{}","dir":"{}","content":"{}"}}"#,
                json_escape(fname), json_escape(&dir_val), json_escape(&text)
            );
            let pdf_path = if ext=="pdf"{format!("{}/{}",esc(&dir_val),esc(fname))}else{String::new()};
            let html = HTML_TPL
                .replace("{{", "{").replace("}}", "}")
                .replace("{title}", &format!("{} &mdash; {}", esc(fname), dir_title))
                .replace("{nav}", &nav)
                .replace("{file}", &esc(fname))
                .replace("{dir}", &esc(&dir_val))
                .replace("{pdf_file}", &pdf_path)
                .replace("{content}", &content_html)
                .replace("{raw_content}", &raw_json);
            send(&mut s, 200, "text/html;charset=utf-8", html.as_bytes());
        } else {
            send_404(&mut s);
        }
    } else {
        let dir_param = q.get("dir").map(|s| s.as_str()).unwrap_or("");
        // Try serving as static file for image extensions
        if path.ends_with(".png") || path.ends_with(".jpg") || path.ends_with(".gif") || path.ends_with(".svg") || path.ends_with(".ico") {
            // 优先使用 dir 参数（来自 sidebar 链接直接导航）
            let mut sfp = if !dir_param.is_empty() {
                served.join(dir_param).join(&path)
            } else {
                served.join(&path)
            };
            // Try CWD's ./docs/ as fallback (for auto-start with D:/ serving)
            if !sfp.is_file() && Path::new("./docs").join(&path).is_file() {
                sfp = Path::new("./docs").join(&path);
            }
            if !sfp.is_file() && path.starts_with("docs/") {
                let stripped = path.strip_prefix("docs/").unwrap_or(&path);
                sfp = served.join(stripped);
                if !sfp.is_file() && Path::new("./docs").join(stripped).is_file() {
                    sfp = Path::new("./docs").join(stripped);
                }
            }
            if sfp.is_file() {
                use std::io::Read;
                let mut sfbuf = Vec::new();
                if let Ok(mut sf) = std::fs::File::open(&sfp) {
                    if sf.read_to_end(&mut sfbuf).is_ok() {
                        let sext = path.rsplit('.').next().unwrap_or("").to_lowercase();
                        let sct = match sext.as_str() { "png" => "image/png", "jpg"|"jpeg" => "image/jpeg", "gif" => "image/gif", "svg" => "image/svg+xml", _ => "application/octet-stream" };
                        send(&mut s, 200, sct, &sfbuf);
                        return;
                    }
                }
            }
        }
        send_404(&mut s);
    }
}

fn nav_html(served: &Path, proj: &Path, q: &HashMap<String, String>, base_path: &str) -> String {
    let cr = cur_root(served, proj, q);
    let mut p: Vec<String> = Vec::new();
    p.push(format!("<h3>{}</h3>", esc(cr.file_name().and_then(|n| n.to_str()).unwrap_or("?"))));
    // parent button: allow going up to proj
    if cr != proj {
        if let Some(parent) = cr.parent() {
            let dp = rel_p(parent, served, proj);
            let name = parent.file_name().and_then(|n| n.to_str()).unwrap_or("?");
            p.push(format!("<a href='{}/?dir={}' class='parent-btn'>&#8593; 上一级: {}/</a>", base_path, esc(&dp), esc(name)));
        }
    }
    // subdirs
    if let Ok(entries) = fs::read_dir(&cr) {
        let mut subs: Vec<_> = entries.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()).collect();
        subs.sort_by(|a, b| {
            let an = a.file_name().to_string_lossy().to_lowercase();
            let bn = b.file_name().to_string_lossy().to_lowercase();
            let a_is_chinese = an.chars().any(|c| c as u32 > 0x4E00);
            let b_is_chinese = bn.chars().any(|c| c as u32 > 0x4E00);
            if a_is_chinese && !b_is_chinese { std::cmp::Ordering::Greater }
            else if !a_is_chinese && b_is_chinese { std::cmp::Ordering::Less }
            else { an.cmp(&bn) }
        });
        if !subs.is_empty() {
            p.push("<h3>&#128193; 目录</h3>".into());
            for e in &subs {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') { continue; }
                let dp = rel_p(&e.path(), served, proj);
                let url_dir = url_encode(&dp);
                p.push(format!("<a href='{}/?dir={}'>&#128193; {}/</a>", base_path, url_dir, esc(&name)));
            }
        }
    }
    // supported files
    if let Ok(entries) = fs::read_dir(&cr) {
        let mut files: Vec<_> = entries.filter_map(|e| e.ok())
            .filter(|e| e.path().is_file() && is_supported(&e.path()))
            .collect();
        files.sort_by(|a, b| {
            let an = a.file_name().to_string_lossy().to_lowercase();
            let bn = b.file_name().to_string_lossy().to_lowercase();
            an.cmp(&bn)
        });
        if !files.is_empty() {
            p.push("<h3>&#128196; 文件</h3>".into());
            for e in &files {
                let name = e.file_name().to_string_lossy().to_string();
                let dp = rel_p(&cr, served, proj);
                let qs = if dp.is_empty() { String::new() } else { format!("?dir={}", url_encode(&dp)) };
                // href 不含 base_path, 因为 dir 已包含完整目录上下文
                // JS 从 href 提取 filename, 与 dir 拼接 = 正确路径
                let href = if dp.is_empty() {
                    format!("{}/{}", base_path, url_encode(&name))
                } else {
                    format!("{}?dir={}", url_encode(&name), url_encode(&dp))
                };
                p.push(format!("<a href='{}'>{}</a>", href, esc(&name)));
            }
        }
    }
    p.join("\n")
}

fn cur_root(served: &Path, proj: &Path, q: &HashMap<String, String>) -> PathBuf {
    let dir_raw = q.get("dir").map(|s| s.as_str()).unwrap_or("");
    let dir = if dir_raw.is_empty() { dir_raw } else { &url_decode(dir_raw) };
    if dir.is_empty() || dir == "." { return served.to_path_buf(); }
    let resolved = served.join(dir).canonicalize().unwrap_or_else(|_| served.to_path_buf());
    if resolved.starts_with(proj) { resolved } else { proj.to_path_buf() }
}

fn rel_p(target: &Path, served: &Path, proj: &Path) -> String {
    let t = target.canonicalize().unwrap_or_else(|_| target.to_path_buf());
    let b = served.canonicalize().unwrap_or_else(|_| served.to_path_buf());
    if t == b { return String::new(); }
    if let Ok(rel) = t.strip_prefix(&b) {
        rel.to_string_lossy().replace('\\', "/")
    } else {
        // above served dir
        let t_abs = t.to_string_lossy().to_string();
        let b_abs = b.to_string_lossy().to_string();
        let mut td: Vec<&str> = t_abs.split(&['\\', '/']).filter(|s| !s.is_empty()).collect();
        let mut bd: Vec<&str> = b_abs.split(&['\\', '/']).filter(|s| !s.is_empty()).collect();
        // find common prefix
        let mut i = 0;
        while i < td.len() && i < bd.len() && td[i] == bd[i] { i += 1; }
        let mut result = String::new();
        for _ in i..bd.len() { result.push_str("../"); }
        for j in i..td.len() { result.push_str(td[j]); if j < td.len() - 1 { result.push('/'); } }
        result
    }
}

fn walk(dir: &Path, name: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                if let Some(found) = walk(&p, name) { return Some(found); }
            } else if p.is_file() && p.file_name().and_then(|n| n.to_str()) == Some(name) {
                return Some(p);
            }
        }
    }
    None
}

/// Protect fenced code blocks tagged ```html before markdown parsing.
/// Their content is pulled out verbatim so it renders as real HTML later
/// (instead of being escaped inside <pre><code>).
/// Returns (protected_text, list_of_raw_html_fragments).
fn protect_html_fences(text: &str) -> (String, Vec<String>) {
    let mut exprs: Vec<String> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_open = trimmed.starts_with("```") && {
            let info = trimmed[3..].trim();
            !info.is_empty() && info.split_whitespace().next().unwrap_or("").eq_ignore_ascii_case("html")
        };
        if is_open {
            let mut j = i + 1;
            let mut closed = false;
            while j < lines.len() {
                if lines[j].trim().starts_with("```") {
                    closed = true;
                    break;
                }
                j += 1;
            }
            if closed {
                let content = lines[i + 1..j].join("\n");
                exprs.push(content);
                out.push(format!("<!--RAWHTML{}-->", exprs.len() - 1));
                i = j + 1;
                continue;
            }
        }
        out.push(lines[i].to_string());
        i += 1;
    }
    (out.join("\n"), exprs)
}

/// Restore ```html fenced fragments that were pulled out by protect_html_fences.
/// The placeholder is an HTML comment emitted verbatim by pulldown-cmark,
/// so replacing it with the raw fragment injects real HTML.
fn restore_html(html: &str, exprs: &[String]) -> String {
    let mut result = html.to_string();
    for (i, expr) in exprs.iter().enumerate() {
        let placeholder = format!("<!--RAWHTML{}-->", i);
        result = result.replace(&placeholder, expr);
    }
    result
}

/// Protect LaTeX $...$ and $$...$$ before markdown parsing.
/// Returns (protected_text, list_of_expressions).
fn protect_latex(text: &str) -> (String, Vec<String>) {
    let mut exprs: Vec<String> = Vec::new();
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // ---- display math \[...\] ----
        if i + 1 < len && chars[i] == '\\' && chars[i + 1] == '[' {
            let start = i;
            i += 2;
            let mut found = false;
            while i + 1 < len {
                if chars[i] == '\\' && chars[i + 1] == ']' {
                    i += 2;
                    found = true;
                    break;
                }
                i += 1;
            }
            if found {
                let expr: String = chars[start..i].iter().collect();
                exprs.push(expr);
                out.push_str(&format!("<!--LATEX{}-->", exprs.len() - 1));
                continue;
            }
            out.push_str("\\[");
            i = start + 2;
            continue;
        }

        // ---- inline math \(...\) ----
        if i + 1 < len && chars[i] == '\\' && chars[i + 1] == '(' {
            let start = i;
            i += 2;
            let mut found = false;
            while i + 1 < len {
                if chars[i] == '\\' && chars[i + 1] == ')' {
                    i += 2;
                    found = true;
                    break;
                }
                i += 1;
            }
            if found {
                let expr: String = chars[start..i].iter().collect();
                exprs.push(expr);
                out.push_str(&format!("<!--LATEX{}-->", exprs.len() - 1));
                continue;
            }
            out.push_str("\\(");
            i = start + 2;
            continue;
        }

        // ---- display math $$...$$ (must check before single $) ----
        if i + 1 < len && chars[i] == '$' && chars[i + 1] == '$' {
            let start = i;
            i += 2;
            let mut found = false;
            while i + 1 < len {
                if chars[i] == '$' && chars[i + 1] == '$' {
                    i += 2;
                    found = true;
                    break;
                }
                i += 1;
            }
            if found {
                let expr: String = chars[start..i].iter().collect();
                exprs.push(expr);
                out.push_str(&format!("<!--LATEX{}-->", exprs.len() - 1));
                continue;
            }
            // unclosed $$ → output literally, don't consume
            out.push_str("$$");
            i = start + 2;
            continue;
        }

        // ---- inline math $...$ (single $) ----
        if chars[i] == '$' {
            // escaped \$ → literal $
            if i > 0 && chars[i - 1] == '\\' {
                out.push('$');
                i += 1;
                continue;
            }
            let start = i;
            i += 1;
            let mut found = false;
            while i < len {
                if chars[i] == '\\' && i + 1 < len && chars[i + 1] == '$' {
                    i += 2;
                    continue;
                }
                if chars[i] == '$' {
                    i += 1;
                    found = true;
                    break;
                }
                if chars[i] == '\n' {
                    break;
                }
                i += 1;
            }
            if found {
                let expr: String = chars[start..i].iter().collect();
                exprs.push(expr);
                out.push_str(&format!("<!--LATEX{}-->", exprs.len() - 1));
                continue;
            }
            // unclosed $ → output literally
            out.push('$');
            i = start + 1;
            continue;
        }

        // ---- LaTeX environment \begin{env}...\end{env} ----
        if i + 7 < len && chars[i] == '\\' && chars[i + 1] == 'b' && chars[i + 2] == 'e'
            && chars[i + 3] == 'g' && chars[i + 4] == 'i' && chars[i + 5] == 'n'
            && chars[i + 6] == '{' {
            // Find the environment name
            let mut j = i + 7;
            let mut env = String::new();
            while j < len && chars[j] != '}' {
                env.push(chars[j]);
                j += 1;
            }
            if j < len {
                j += 1; // skip }
                let end_tag = format!("\\end{{{}}}", env);
                // Search for \end{env} (字符安全: chars[]是字符索引, text[]是字节索引)
                let tail: String = chars[j..].iter().collect();
                if let Some(end_pos) = tail.find(&end_tag) {
                    let end_abs = j + tail[..end_pos].chars().count() + end_tag.chars().count();
                    let expr: String = chars[i..end_abs].iter().collect();
                    exprs.push(expr);
                    out.push_str(&format!("<!--LATEX{}-->", exprs.len() - 1));
                    i = end_abs;
                    continue;
                }
            }
            // unmatched \begin → output literally
            out.push('\\');
            i += 1;
            continue;
        }

        out.push(chars[i]);
        i += 1;
    }

    (out, exprs)
}

/// Restore LaTeX expressions after markdown→HTML conversion.
/// Wraps \begin{...}...\end{...} and \[...\] in $$ so KaTeX renders them.
fn restore_latex(html: &str, exprs: &[String]) -> String {
    let mut result = html.to_string();
    if exprs.is_empty() {
        return result;
    }
    for (i, expr) in exprs.iter().enumerate() {
        let placeholder = format!("<!--LATEX{}-->", i);
        let replacement = if expr.starts_with("\\begin{") {
            format!("$${}$$", expr)
        } else if expr.starts_with("\\[") && expr.ends_with("\\]") {
            // \[...\] → $$...$$ for KaTeX
            let inner = &expr[2..expr.len()-2];
            format!("$${}$$", inner)
        } else if expr.starts_with("\\(") && expr.ends_with("\\)") {
            let inner = &expr[2..expr.len()-2];
            format!("${}$", inner)
        } else {
            expr.clone()
        };
        // Escape < and > inside LaTeX to prevent browser HTML parser from consuming them
        let escaped = replacement.replace('<', "&lt;").replace('>', "&gt;");
        result = result.replace(&placeholder, &escaped);
    }
    result
}

/// Strip leading whitespace from every line to prevent unintentional code blocks.
/// Tab-indented markdown (common in note-taking) should render as regular paragraphs.
fn strip_indent(text: &str) -> String {
    text.lines()
        .map(|line| line.trim_start())
        .collect::<Vec<_>>()
        .join("\n")
}

fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut b = [0u8; 3];
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i+1] as char).to_digit(16).unwrap_or(0) as u8;
            let lo = (bytes[i+2] as char).to_digit(16).unwrap_or(0) as u8;
            b[0] = (hi << 4) | lo;
            // try utf8 decode: 1-3 bytes
            let n = if b[0] & 0x80 == 0 { 1 }
                else if b[0] & 0xE0 == 0xC0 { 2 }
                else if b[0] & 0xF0 == 0xE0 { 3 }
                else { 1 };
            if n > 1 {
                for j in 1..n {
                    if i + j*3 + 2 < bytes.len() && bytes[i + j*3] == b'%' {
                        let hi2 = (bytes[i+1+j*3] as char).to_digit(16).unwrap_or(0) as u8;
                        let lo2 = (bytes[i+2+j*3] as char).to_digit(16).unwrap_or(0) as u8;
                        b[j] = (hi2 << 4) | lo2;
                    } else { return s.to_string(); }
                }
            }
            out.push_str(std::str::from_utf8(&b[..n]).unwrap_or("?"));
            i += n * 3;
        } else if bytes[i] == b'+' {
            out.push(' ');
            i += 1;
        } else {
            // raw non-ASCII byte: use lossy conversion
            let raw = &bytes[i..];
            let (ch, n) = if raw.len() >= 3 && (raw[0] & 0xF0) == 0xE0
                && (raw[1] & 0xC0) == 0x80 && (raw[2] & 0xC0) == 0x80 {
                // 3-byte UTF-8
                let c = std::str::from_utf8(&raw[..3]).unwrap_or("?");
                (c.to_string(), 3)
            } else if raw.len() >= 2 && (raw[0] & 0xE0) == 0xC0
                && (raw[1] & 0xC0) == 0x80 {
                let c = std::str::from_utf8(&raw[..2]).unwrap_or("?");
                (c.to_string(), 2)
            } else {
                (std::str::from_utf8(&raw[..1]).unwrap_or("?").to_string(), 1)
            };
            out.push_str(&ch);
            i += n;
        }
    }
    out
}

fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b'/' => out.push('/'),  // keep path separators
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// Extract a JSON string value by key. Handles \\n \\r \\t \\\\ \\" escapes.
fn json_str(body: &str, key: &str) -> Option<String> {
    let search = format!("\"{}\"", key);
    let pos = body.find(&search)?;
    let after = &body[pos + search.len()..];
    let colon = after.find(':')?;
    let after = after[colon + 1..].trim_start();
    if !after.starts_with('"') { return None; }
    let mut result = String::new();
    let chars: Vec<char> = after[1..].chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            match chars[i + 1] {
                '"' => result.push('"'),
                '\\' => result.push('\\'),
                'n' => result.push('\n'),
                'r' => result.push('\r'),
                't' => result.push('\t'),
                '/' => result.push('/'),
                c => { result.push('\\'); result.push(c); }
            }
            i += 2;
        } else if chars[i] == '"' {
            break;
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    Some(result)
}

/// JSON-string-escape for embedding raw content
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out
}

/// Render .ipynb JSON as notebook cells
fn render_ipynb(text: &str) -> String {
    let mut html = String::from("<article class=\"notebook\">");
    // Find cell_type occurrences
    let mut pos = 0;
    while let Some(cell_start) = text[pos..].find("\"cell_type\"") {
        let abs_start = pos + cell_start;
        let after = &text[abs_start..];
        // get cell_type value
        let ct_start = after.find(':').map(|i| &after[i + 1..]).and_then(|s| s.trim_start().strip_prefix('"'));
        if let Some(ct_val) = ct_start {
            let ct_end = ct_val.find('"').unwrap_or(0);
            let cell_type = &ct_val[..ct_end];
            // Find source array
            let after_ct = &ct_val[ct_end..];
            if let Some(src_idx) = after_ct.find("\"source\"") {
                let src_section = &after_ct[src_idx..];
                if let Some(arr_start) = src_section.find('[') {
                    let arr_content = &src_section[arr_start..];
                    // Find matching ]
                    let mut depth = 0;
                    let mut arr_end = 0;
                    for (i, c) in arr_content.char_indices() {
                        if c == '[' { depth += 1; }
                        else if c == ']' { depth -= 1; if depth == 0 { arr_end = i; break; } }
                    }
                    let arr = &arr_content[1..arr_end];
                    // Extract strings with unescaping
                    let lines: Vec<String> = {
                        let mut ls = Vec::new();
                        let mut ap = 0;
                        while let Some(q) = arr[ap..].find('"') {
                            let inner = &arr[ap + q + 1..];
                            let mut esc = false;
                            let mut eq = 0;
                            for (i, c) in inner.char_indices() {
                                if esc { esc = false; eq = i; continue; }
                                if c == '\\' { esc = true; eq = i; continue; }
                                if c == '"' { eq = i; break; }
                                eq = i;
                            }
                            let raw = &inner[..eq];
                            // Unescape \n \r \t \\ \"
                            let mut s = String::new();
                            let mut ec = false;
                            for ch in raw.chars() {
                                if ec {
                                    match ch { 'n' => s.push('\n'), 'r' => s.push('\r'), 't' => s.push('\t'), '\\' => s.push('\\'), '"' => s.push('"'), '/' => s.push('/'), c => { s.push('\\'); s.push(c); } }
                                    ec = false;
                                } else if ch == '\\' { ec = true; }
                                else { s.push(ch); }
                            }
                            if ec { s.push('\\'); }
                            ls.push(s);
                            ap = ap + q + 1 + eq + 1;
                        }
                        ls
                    };
                    let source = lines.join("");
                    html.push_str("<div class=\"nb-cell\">");
                    if cell_type == "markdown" {
                        let mut opts = Options::empty();
                        opts.insert(Options::ENABLE_TABLES);
                        opts.insert(Options::ENABLE_STRIKETHROUGH);
                        let (pro, lex) = protect_latex(&source);
                        let (pro2, hfx) = protect_html_fences(&pro);
                        let parser = Parser::new_ext(&pro2, opts);
                        let mut buf = String::new();
                        html::push_html(&mut buf, parser);
                        let restored = restore_latex(&buf, &lex);
                        html.push_str(&format!("<div class=\"nb-md\">{}</div>", restore_html(&restored, &hfx)));
                    } else {
                        // code cell
                        html.push_str(&format!("<pre><code class=\"language-python\">{}</code></pre>", esc(&source)));
                    }
                    html.push_str("</div>");
                }
            }
            pos = abs_start + ct_end;
            if cell_type == "markdown" { pos = abs_start + after_ct.find("\"source\"").unwrap_or(0) + 30; }
        } else {
            pos = abs_start + 1;
        }
    }
    html.push_str("</article>");
    html
}

fn render_tex(text: &str) -> String {
    let escaped = esc(text);
    format!(
        r#"<article class="tex-preview"><div class="tex-source">{}</div></article>"#,
        escaped
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace("'", "&#x27;")
}

fn send(s: &mut TcpStream, status: u16, ct: &str, data: &[u8]) {
    let sl = match status { 200 => "200 OK", 404 => "404 Not Found", _ => "200 OK" };
    let h = format!("HTTP/1.0 {}\r\nContent-Type:{}\r\nContent-Length:{}\r\nCache-Control:no-store\r\nAccess-Control-Allow-Origin:*\r\n\r\n", sl, ct, data.len());
    let _ = s.write_all(h.as_bytes());
    let _ = s.write_all(data);
}

fn send_404(s: &mut TcpStream) {
    send(s, 404, "text/plain;charset=utf-8", b"File not found");
}
