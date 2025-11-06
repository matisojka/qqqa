use nu_ansi_term::{Color, Style};
use std::io::Write as _;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

/// Render our pseudo-XML to ANSI-colored text.
/// Supported tags: <bold>, <cmd>, <info>, <file>, <warn>, <code>, <br/>
pub fn render_xmlish_to_ansi(input: &str) -> String {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Tag {
        Bold,
        Cmd,
        Info,
        File,
        Warn,
        Code,
    }

    fn style_for_stack(stack: &[Tag]) -> Style {
        let mut st = Style::new();
        for t in stack {
            match t {
                Tag::Bold => {
                    st = st.bold();
                }
                Tag::Cmd => {
                    st = st.fg(Color::Green);
                }
                Tag::Info => {
                    st = st.fg(Color::Cyan);
                }
                Tag::File => {
                    st = st.fg(Color::Magenta);
                }
                Tag::Warn => {
                    st = st.fg(Color::Yellow);
                }
                Tag::Code => {
                    st = st.fg(Color::Cyan);
                }
            }
        }
        st
    }

    fn unescape(s: &str) -> String {
        s.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&")
    }

    let mut out = String::new();
    let mut stack: Vec<Tag> = Vec::new();
    let mut cursor = 0usize;
    while let Some(start) = input[cursor..].find('<') {
        let start_abs = cursor + start;
        // Emit text before tag
        if start_abs > cursor {
            let seg = &input[cursor..start_abs];
            let text = unescape(seg);
            if stack.is_empty() {
                out.push_str(&text);
            } else {
                out.push_str(&style_for_stack(&stack).paint(text).to_string());
            }
        }
        // Find end of tag
        if let Some(end_rel) = input[start_abs..].find('>') {
            let end_abs = start_abs + end_rel;
            let raw = &input[start_abs + 1..end_abs].trim();
            // Advance cursor after '>'
            cursor = end_abs + 1;

            // Handle self-closing br
            let raw_no_space = raw.replace(' ', "");
            let tag_id = raw_no_space.trim_end_matches('/').to_ascii_lowercase();
            if tag_id == "br" {
                out.push('\n');
                continue;
            }

            // Closing tag
            if raw.starts_with('/') {
                let name = raw[1..].trim().to_ascii_lowercase();
                let tag = match name.as_str() {
                    // supported
                    "bold" | "strong" | "b" => Some(Tag::Bold),
                    "cmd" => Some(Tag::Cmd),
                    "info" => Some(Tag::Info),
                    "file" => Some(Tag::File),
                    "warn" => Some(Tag::Warn),
                    "code" => Some(Tag::Code),
                    // common HTML tags we ignore (no style impact)
                    "p" | "span" | "div" | "em" | "i" | "u" => None,
                    _ => None,
                };
                if let Some(t) = tag {
                    // pop the most recent matching tag
                    if let Some(pos) = stack.iter().rposition(|x| *x == t) {
                        stack.remove(pos);
                    }
                } // else: ignore unknown closing tag
                continue;
            }

            // Opening tag
            let name = raw.to_ascii_lowercase();
            match name.as_str() {
                "bold" | "strong" | "b" => stack.push(Tag::Bold),
                "cmd" => stack.push(Tag::Cmd),
                "info" => stack.push(Tag::Info),
                "file" => stack.push(Tag::File),
                "warn" => stack.push(Tag::Warn),
                "code" => stack.push(Tag::Code),
                // Ignore unknown/neutral tags instead of emitting raw
                _ => { /* no-op */ }
            }
        } else {
            // No closing '>' found: emit the rest literally
            out.push_str(&input[start_abs..]);
            cursor = input.len();
            break;
        }
    }
    // Emit any trailing text
    if cursor < input.len() {
        let seg = &input[cursor..];
        let text = unescape(seg);
        if stack.is_empty() {
            out.push_str(&text);
        } else {
            out.push_str(&style_for_stack(&stack).paint(text).to_string());
        }
    }

    out
}

/// Print a transient status to stderr (no spinner to keep deps small).
pub fn status_thinking() {
    eprintln!("{}", Color::Yellow.paint("Thinking…"));
}

/// Print streamed token to stdout. We avoid buffering to keep latency low.
pub fn print_stream_token(token: &str) {
    print!("{}", token);
    let _ = std::io::Write::flush(&mut std::io::stdout());
}

/// Print a full, non-streamed assistant message.
pub fn print_assistant_text(text: &str, raw: bool) {
    if raw {
        println!("{}", text);
    } else {
        let rendered = render_xmlish_to_ansi(text);
        let compacted = compact_blank_lines(&rendered);
        println!("{}", compacted.trim_end());
    }
}

/// Handle to a simple loading animation (cyclic dots) printed to stderr.
pub struct LoadingAnimation {
    stop: Arc<AtomicBool>,
    handle: Option<thread::JoinHandle<()>>,
}

impl LoadingAnimation {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        // Clear the line
        eprint!("\r        \r");
        let _ = std::io::stderr().flush();
    }
}

impl Drop for LoadingAnimation {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
        eprint!("\r        \r");
        let _ = std::io::stderr().flush();
    }
}

/// Start the cyclic dot animation on stderr. Returns a handle that stops it when dropped.
pub fn start_loading_animation() -> LoadingAnimation {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_cl = stop.clone();
    let handle = thread::spawn(move || {
        let mut n: u8 = 1;
        while !stop_cl.load(Ordering::SeqCst) {
            let dots = match n {
                1 => ".",
                2 => "..",
                _ => "...",
            };
            eprint!("\r{}", dots);
            let _ = std::io::stderr().flush();
            n = if n >= 3 { 1 } else { n + 1 };
            thread::sleep(Duration::from_millis(300));
        }
    });
    LoadingAnimation {
        stop,
        handle: Some(handle),
    }
}

/// Reduce runs of blank lines to a single blank line and normalize newlines.
pub fn compact_blank_lines(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_blank = false;
    for line in input.replace('\r', "").split('\n') {
        let is_blank = line.trim().is_empty();
        if is_blank {
            if !last_blank {
                out.push('\n');
                last_blank = true;
            }
        } else {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(line);
            out.push('\n');
            last_blank = false;
        }
    }
    out
}
