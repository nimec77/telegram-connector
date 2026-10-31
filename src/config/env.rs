//! Environment-variable expansion (`${VAR}`) for config TOML values.
//!
//! Unit of `config` (LM-6).

pub(crate) fn expand_env_vars(value: &str) -> anyhow::Result<String> {
    expand_env_vars_with(value, |name| std::env::var(name))
}

/// Pure expansion seam: environment access is supplied by the caller.
pub(super) fn expand_env_vars_with(
    value: &str,
    mut lookup: impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> anyhow::Result<String> {
    let lines: Vec<String> = value
        .split('\n')
        .enumerate()
        .map(|(line_index, line)| {
            if line.trim_start().starts_with('#') {
                Ok(line.to_string())
            } else {
                expand_env_vars_in_line(line, line_index + 1, &mut lookup)
            }
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(lines.join("\n"))
}

fn expand_env_vars_in_line(
    value: &str,
    line_number: usize,
    lookup: &mut impl FnMut(&str) -> Result<String, std::env::VarError>,
) -> anyhow::Result<String> {
    let mut result = value.to_string();
    let mut search_from = 0;

    while let Some(rel_start) = result[search_from..].find("${") {
        let start = search_from + rel_start;
        if let Some(end_offset) = result[start..].find('}') {
            let end = start + end_offset;
            let var_name = &result[start + 2..end];
            // VarError::NotUnicode owns the raw value. Never retain it as a source:
            // Debug/error-chain rendering would disclose non-Unicode credentials.
            let var_value = lookup(var_name).map_err(|error| {
                let condition = match error {
                    std::env::VarError::NotPresent => "not found",
                    std::env::VarError::NotUnicode(_) => "is not valid Unicode",
                };
                anyhow::anyhow!(
                    "Environment variable '{}' {}. Referenced in config as '${{{}}}' at line {}. Ensure it is set in the process environment.",
                    var_name, condition, var_name, line_number
                )
            })?;

            // Check if this is a quoted value that's ONLY an env var: "= \"${VAR}\""
            // If so and the value is purely numeric (digits only), unquote for TOML parsing
            let is_quoted_only_env_var = start >= 1
                && result.as_bytes().get(start - 1) == Some(&b'"')
                && result.as_bytes().get(end + 1) == Some(&b'"');

            // Only unquote if value is purely digits (no +/- signs, no decimals)
            // This ensures phone numbers like "+1234567890" stay as strings
            let is_pure_integer =
                !var_value.is_empty() && var_value.chars().all(|c| c.is_ascii_digit());

            if is_quoted_only_env_var && is_pure_integer {
                // Replace including surrounding quotes: "12345" -> 12345
                result.replace_range((start - 1)..=(end + 1), &var_value);
                search_from = start - 1 + var_value.len();
            } else {
                result.replace_range(start..=end, &var_value);
                search_from = start + var_value.len();
            }
        } else {
            break;
        }
    }

    Ok(result)
}
