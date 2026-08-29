//! A DSR query in the stream produces the correct cursor-position reply.

use laura::dsr_reply;

#[test]
fn answers_cursor_position_query() {
    // ConPTY's query embedded in a chunk of normal output.
    let chunk = b"some output\x1b[6nmore output";
    assert_eq!(dsr_reply(chunk), Some(&b"\x1b[1;1R"[..]));
}

#[test]
fn no_reply_without_full_query() {
    assert_eq!(dsr_reply(b"plain child output, no query here"), None);
    assert_eq!(dsr_reply(b"\x1b[6"), None);
}
