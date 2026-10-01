use projman_core::{Error, Result};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub const MAX_FRAME: usize = 4 * 1024 * 1024;
/// Drain oversized frames without retaining them; the next valid request remains usable.
pub async fn next_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Option<Result<String>>> {
    let mut frame = Vec::new();
    let mut oversized = false;
    loop {
        let bytes = reader.fill_buf().await?;
        if bytes.is_empty() {
            if frame.is_empty() && !oversized {
                return Ok(None);
            }
            break;
        }
        let newline = bytes.iter().position(|b| *b == b'\n');
        let len = newline.map_or(bytes.len(), |i| i + 1);
        if !oversized && frame.len() + len <= MAX_FRAME {
            frame.extend_from_slice(&bytes[..len]);
        } else {
            oversized = true;
            frame.clear();
        }
        reader.consume(len);
        if newline.is_some() {
            break;
        }
    }
    if oversized {
        return Ok(Some(Err(Error::validation("Request exceeds 4 MiB"))));
    }
    Ok(Some(
        String::from_utf8(frame).map_err(|_| Error::validation("Request is not UTF-8")),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;
    #[tokio::test]
    async fn oversized_frame_does_not_consume_next_request() {
        let mut data = vec![b'x'; MAX_FRAME + 5];
        data.extend_from_slice(b"\n{\"ok\":true}\n");
        let mut reader = BufReader::with_capacity(13, data.as_slice());
        assert!(next_frame(&mut reader).await.unwrap().unwrap().is_err());
        assert_eq!(
            next_frame(&mut reader).await.unwrap().unwrap().unwrap(),
            "{\"ok\":true}\n"
        );
        assert!(next_frame(&mut reader).await.unwrap().is_none());
    }
}
