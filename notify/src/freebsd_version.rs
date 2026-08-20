/// Native inotify shipped in FreeBSD 14.5 and later (including 15.0+).
const FREEBSD_INOTIFY_MIN: (u32, u32) = (14, 5);

/// Parse `freebsd-version -u` output, e.g. `14.5-RELEASE-p1` → `(14, 5)`.
fn parse_freebsd_release(output: &str) -> Option<(u32, u32)> {
    let (major_str, rest) = output.trim().split_once('.')?;
    let major = major_str.parse().ok()?;
    let minor_len = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let minor = rest.get(..minor_len)?.parse().ok()?;
    Some((major, minor))
}
