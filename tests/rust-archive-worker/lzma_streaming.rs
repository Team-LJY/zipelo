use super::*;
use std::io::{Read, Seek};

#[test]
fn distant_dictionary_reads_and_overlap_preserve_history() {
    let mut file = crate::spool::temporary().unwrap();
    {
        let mut out = History::new(&mut file, 64 * 1024 * 1024).unwrap();
        for n in 0..144 {
            out.extend(&vec![n as u8; 65536]).unwrap();
        }
        assert_eq!(
            out.previous(0, 9 * 1024 * 1024 - 1, 64 * 1024 * 1024),
            Ok(0)
        );
        out.copy(0, 9 * 1024 * 1024 - 1, 64 * 1024 * 1024, 273)
            .unwrap();
        assert_eq!(
            out.previous(out.len(), 0, 64 * 1024 * 1024),
            Err(ErrorCode::Integrity)
        );
        out.copy(0, 0, 64 * 1024 * 1024, 273).unwrap();
        out.flush().unwrap();
    }
    file.rewind().unwrap();
    let mut tail = [255; 546];
    file.seek(std::io::SeekFrom::End(-546)).unwrap();
    file.read_exact(&mut tail).unwrap();
    assert_eq!(tail, [0; 546]);
}
#[test]
fn streaming_decoder_keeps_cancellation_error() {
    let fixture = include_bytes!("fixtures/7z/lzma2.7z");
    let mut file = crate::spool::temporary().unwrap();
    let mut input = std::io::Cursor::new(&fixture[32..]);
    assert_eq!(
        decode_stream(&mut input, 100, &[0], true, 1000, &mut file, &mut || Err(
            ErrorCode::None
        )),
        Err(ErrorCode::None)
    );
}
