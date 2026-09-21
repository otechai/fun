//! The `fun list` table: pretty and aligned on a terminal, tab-separated (no header) when piped.

pub struct Row {
    pub name: String,
    pub language: String,
    pub created: String,
    pub runs: u64,
    pub lines: usize,
    pub bytes: u64,
}

const HEADERS: [&str; 6] = ["NAME", "LANGUAGE", "CREATED", "RUNS", "LINES", "SIZE (MB)"];
const RIGHT: [bool; 6] = [false, false, false, true, true, true];

/// Megabytes (10^6 bytes) to four places; a script too small for that says so instead of showing 0.0000.
pub fn mb(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if bytes > 0 && mb < 0.000_05 {
        "<0.0001".into()
    } else {
        format!("{mb:.4}")
    }
}

/// Lines as an editor counts them: a final line without a newline still counts.
pub fn count_lines(data: &[u8]) -> usize {
    let newlines = data.iter().filter(|&&b| b == b'\n').count();
    newlines + usize::from(data.last().is_some_and(|&b| b != b'\n'))
}

fn cells(row: &Row) -> [String; 6] {
    [
        row.name.clone(),
        row.language.clone(),
        row.created.clone(),
        row.runs.to_string(),
        row.lines.to_string(),
        mb(row.bytes),
    ]
}

pub fn render(rows: &[Row], pretty: bool) -> String {
    let body: Vec<[String; 6]> = rows.iter().map(cells).collect();
    if !pretty {
        return body.iter().map(|r| r.join("\t") + "\n").collect();
    }
    let mut widths = HEADERS.map(str::len);
    for row in &body {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let header = HEADERS.map(String::from);
    std::iter::once(&header)
        .chain(&body)
        .map(|row| {
            let line: Vec<String> = row
                .iter()
                .enumerate()
                .map(|(i, cell)| {
                    let pad = widths[i] - cell.chars().count();
                    if RIGHT[i] {
                        format!("{}{cell}", " ".repeat(pad))
                    } else {
                        format!("{cell}{}", " ".repeat(pad))
                    }
                })
                .collect();
            line.join("  ").trim_end().to_owned() + "\n"
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, lang: &str, runs: u64, lines: usize, bytes: u64) -> Row {
        Row {
            name: name.into(),
            language: lang.into(),
            created: "2026-09-20".into(),
            runs,
            lines,
            bytes,
        }
    }

    #[test]
    fn megabytes() {
        assert_eq!(mb(0), "0.0000");
        assert_eq!(mb(1), "<0.0001");
        assert_eq!(mb(49), "<0.0001");
        assert_eq!(mb(50), "0.0001");
        assert_eq!(mb(1_234), "0.0012");
        assert_eq!(mb(1_500_000), "1.5000");
        assert_eq!(mb(25_000_000), "25.0000");
    }

    #[test]
    fn line_counts() {
        assert_eq!(count_lines(b""), 0);
        assert_eq!(count_lines(b"\n"), 1);
        assert_eq!(count_lines(b"a"), 1);
        assert_eq!(count_lines(b"a\n"), 1);
        assert_eq!(count_lines(b"a\nb"), 2);
        assert_eq!(count_lines(b"a\nb\n\n"), 3);
    }

    #[test]
    fn pretty_table_aligns_numbers_right() {
        let rows = [
            row("backup", "bash", 3, 12, 40),
            row("hi", "python3", 41, 5, 1_234_567),
        ];
        assert_eq!(
            render(&rows, true),
            "NAME    LANGUAGE  CREATED     RUNS  LINES  SIZE (MB)\n\
             backup  bash      2026-09-20     3     12    <0.0001\n\
             hi      python3   2026-09-20    41      5     1.2346\n"
        );
    }

    #[test]
    fn piped_output_is_headerless_tsv() {
        let rows = [row("backup", "bash", 3, 12, 40)];
        assert_eq!(render(&rows, false), "backup\tbash\t2026-09-20\t3\t12\t<0.0001\n");
    }
}
