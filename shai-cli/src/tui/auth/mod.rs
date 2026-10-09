pub mod auth;
pub mod config_env;
pub mod config_list;
pub mod config_model;
pub mod config_providers;

pub use auth::AppAuth;

/// Maximum number of rows an error message may occupy in the auth modals.
const MAX_ERROR_HEIGHT: u16 = 8;

/// Number of rows needed to display `error` wrapped at `width` columns.
///
/// Each input line takes at least one row, plus one extra row per `width`
/// columns it spans. The result is at least 1 and capped at [`MAX_ERROR_HEIGHT`].
pub(super) fn error_height(error: &str, width: u16) -> u16 {
    let width = width.max(1) as usize;
    let lines: usize = error
        .lines()
        .map(|line| line.chars().count().div_ceil(width).max(1))
        .sum();
    (lines.max(1).min(MAX_ERROR_HEIGHT as usize)) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_fits_in_one_row() {
        assert_eq!(error_height("boom", 80), 1);
    }

    #[test]
    fn long_single_line_wraps() {
        assert_eq!(error_height(&"x".repeat(25), 10), 3);
    }

    #[test]
    fn multiline_counts_each_line() {
        let error = "line one\nline two\nline three";
        assert_eq!(error_height(error, 80), 3);
    }

    #[test]
    fn blank_lines_count_as_one_row() {
        assert_eq!(error_height("a\n\nb", 80), 3);
    }

    #[test]
    fn multiline_with_wrapping() {
        let error = format!("{}\n{}", "x".repeat(25), "y".repeat(11));
        // 25 chars at width 10 -> 3 rows, 11 chars -> 2 rows
        assert_eq!(error_height(&error, 10), 5);
    }

    #[test]
    fn empty_error_takes_one_row() {
        assert_eq!(error_height("", 80), 1);
        assert_eq!(error_height("\n", 80), 1);
    }

    #[test]
    fn zero_width_is_treated_as_one() {
        assert_eq!(error_height("abc", 0), 3);
    }

    #[test]
    fn height_is_capped() {
        let error = (0..50)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(error_height(&error, 80), MAX_ERROR_HEIGHT);
    }
}
