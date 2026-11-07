//! Tiny ASCII table renderer used to pretty-print CLI output.
//!
//! The renderer produces plain ASCII (no Unicode box-drawing) so it works in
//! every terminal and is easy to parse in tests.

/// Render a bordered ASCII table.
///
/// `headers` may be empty, in which case no header row is emitted. Rows are
/// padded to the number of columns implied by `headers` (or the widest row).
pub fn render(headers: &[&str], rows: &[Vec<String>]) -> String {
    let ncols = headers
        .len()
        .max(rows.iter().map(|r| r.len()).max().unwrap_or(0));

    if ncols == 0 {
        return String::new();
    }

    // Normalise headers/rows to ncols.
    let headers: Vec<String> = (0..ncols)
        .map(|i| headers.get(i).copied().unwrap_or("").to_string())
        .collect();
    let rows: Vec<Vec<String>> = rows
        .iter()
        .map(|r| {
            let mut r = r.clone();
            while r.len() < ncols {
                r.push(String::new());
            }
            r.truncate(ncols);
            r
        })
        .collect();

    // Column widths.
    let mut widths = vec![0usize; ncols];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = widths[i].max(h.chars().count());
    }
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }
    // At least 1 wide, plus padding.
    for w in &mut widths {
        if *w == 0 {
            *w = 1;
        }
        *w += 2; // padding
    }

    let border = |left: char, mid: char, right: char, fill: char| -> String {
        let mut s = String::new();
        s.push(left);
        for (i, w) in widths.iter().enumerate() {
            s.push_str(&fill.to_string().repeat(*w));
            if i + 1 < widths.len() {
                s.push(mid);
            }
        }
        s.push(right);
        s
    };

    let row_line = |cells: &[String]| -> String {
        let mut s = String::new();
        s.push('|');
        for (i, cell) in cells.iter().enumerate() {
            s.push(' ');
            s.push_str(cell);
            let trailing = widths[i].saturating_sub(cell.chars().count() + 1);
            s.push_str(&" ".repeat(trailing));
            if i + 1 < cells.len() {
                s.push('|');
            }
        }
        s.push('|');
        s
    };

    let top = crate::color::dim(&border('+', '+', '+', '-'));
    let sep = crate::color::dim(&border('+', '+', '+', '-'));
    let bottom = crate::color::dim(&border('+', '+', '+', '-'));

    let mut out = String::new();
    out.push_str(&top);
    out.push('\n');

    let has_header = headers.iter().any(|h| !h.is_empty());
    if has_header {
        out.push_str(&crate::color::bold(&row_line(&headers)));
        out.push('\n');
        out.push_str(&sep);
        out.push('\n');
    }

    for row in &rows {
        out.push_str(&row_line(row));
        out.push('\n');
    }

    out.push_str(&bottom);
    out
}

/// Render a two-column key/value table.
pub fn render_kv(pairs: &[(&str, String)]) -> String {
    let rows: Vec<Vec<String>> = pairs
        .iter()
        .map(|(k, v)| vec![k.to_string(), v.clone()])
        .collect();
    render(&["Key", "Value"], &rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_table() {
        assert_eq!(render(&[], &[]), "");
    }

    #[test]
    fn simple_table() {
        let t = render(&["Name", "State"], &[vec!["foo".into(), "on".into()]]);
        assert!(t.contains("foo"));
        assert!(t.contains("on"));
        assert!(t.starts_with('+'));
    }

    #[test]
    fn kv_table() {
        let t = render_kv(&[("source", "ac".to_string())]);
        assert!(t.contains("source"));
        assert!(t.contains("ac"));
    }
}
