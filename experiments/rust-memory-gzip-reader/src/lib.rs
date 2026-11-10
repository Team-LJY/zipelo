#![forbid(unsafe_code)]

mod budget;
pub use budget::{Budget, CHUNK_SIZE, Limits, Usage};
pub use zlib_rs::InflateError;
use zlib_rs::{Inflate, InflateFlush, Status, crc32::crc32};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    InvalidRequest,
    Header,
    Truncated,
    Boundary,
    Crc,
    Size,
    TrailingData,
    Budget,
    NoProgress,
    Allocation,
    Cancelled,
}

#[derive(Debug)]
pub enum Error<E> {
    Reader(Failure),
    Backend(InflateError),
    Callback(E),
}

impl<E> From<Failure> for Error<E> {
    fn from(value: Failure) -> Self {
        Self::Reader(value)
    }
}

#[derive(Debug)]
pub enum Stop<E> {
    Cancelled,
    Deadline,
    Callback(E),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Start,
    Header,
    BeforeInflate,
    AfterInflate,
    AfterOutput,
    Trailer,
    BeforeReturn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Checkpoint {
    pub stage: Stage,
    pub usage: Usage,
}

pub struct Request {
    pub input: Box<[u8]>,
    pub selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Boundary {
    pub header_bytes: usize,
    pub deflate_bytes: u64,
    pub read_ahead_bytes: usize,
    pub inflated_bytes: u64,
    pub crc32: u32,
    pub member_bytes: usize,
    pub offered_bytes: u64,
}

#[derive(Debug)]
pub struct Completed {
    pub boundary: Boundary,
    pub bytes: Option<Vec<u8>>,
    pub usage: Usage,
}

pub fn read<E>(
    request: Request,
    budget: &mut Budget,
    mut checkpoint: impl FnMut(Checkpoint) -> Result<(), Stop<E>>,
) -> Result<Completed, Error<E>> {
    let mut context = Context {
        budget,
        checkpoint: &mut checkpoint,
    };
    context.check(Stage::Start)?;
    if request.input.len() as u64 > context.budget.limits().input {
        return Err(Failure::Budget.into());
    }
    let mut cursor = Cursor {
        input: &request.input,
        position: 0,
        high_water: 0,
    };
    let header_bytes = header(&mut cursor, &mut context)?;
    let mut bytes = request.selected.then(Vec::new);
    let boundary = {
        let mut decoder = Inflate::new(false, 15);
        decode(
            &mut decoder,
            &mut cursor,
            &mut context,
            header_bytes,
            &mut bytes,
        )?
    };
    // decoder 정리 이후에만 완료 결과를 노출함.
    context.check(Stage::BeforeReturn)?;
    Ok(Completed {
        boundary,
        bytes,
        usage: context.budget.usage(),
    })
}

struct Context<'a, F> {
    budget: &'a mut Budget,
    checkpoint: &'a mut F,
}

impl<E, F: FnMut(Checkpoint) -> Result<(), Stop<E>>> Context<'_, F> {
    fn check(&mut self, stage: Stage) -> Result<(), Error<E>> {
        (self.checkpoint)(Checkpoint {
            stage,
            usage: self.budget.usage(),
        })
        .map_err(|stop| match stop {
            Stop::Cancelled => Error::Reader(Failure::Cancelled),
            Stop::Deadline => Error::Reader(Failure::Budget),
            Stop::Callback(error) => Error::Callback(error),
        })?;
        self.budget.check()?;
        Ok(())
    }
}

struct Cursor<'a> {
    input: &'a [u8],
    position: usize,
    high_water: usize,
}

impl Cursor<'_> {
    fn account(&mut self, end: usize, budget: &mut Budget) -> Result<(), Failure> {
        if end > self.input.len() {
            return Err(Failure::Boundary);
        }
        if end > self.high_water {
            budget.input(end - self.high_water)?;
            self.high_water = end;
        }
        Ok(())
    }

    fn byte<E, F: FnMut(Checkpoint) -> Result<(), Stop<E>>>(
        &mut self,
        context: &mut Context<'_, F>,
        stage: Stage,
    ) -> Result<u8, Error<E>> {
        context.check(stage)?;
        let value = *self.input.get(self.position).ok_or(Failure::Truncated)?;
        let end = self.position.checked_add(1).ok_or(Failure::Boundary)?;
        self.account(end, context.budget)?;
        self.position = end;
        Ok(value)
    }
}

fn header<E, F: FnMut(Checkpoint) -> Result<(), Stop<E>>>(
    cursor: &mut Cursor<'_>,
    context: &mut Context<'_, F>,
) -> Result<usize, Error<E>> {
    let mut crc = 0;
    fn next<E, F: FnMut(Checkpoint) -> Result<(), Stop<E>>>(
        cursor: &mut Cursor<'_>,
        context: &mut Context<'_, F>,
        crc: &mut u32,
    ) -> Result<u8, Error<E>> {
        context.check(Stage::Header)?;
        context.budget.metadata()?;
        let value = cursor.byte(context, Stage::Header)?;
        *crc = crc32(*crc, &[value]);
        Ok(value)
    }
    for expected in [0x1f, 0x8b, 8] {
        if next(cursor, context, &mut crc)? != expected {
            return Err(Failure::Header.into());
        }
    }
    let flags = next(cursor, context, &mut crc)?;
    if flags & 0xe0 != 0 {
        return Err(Failure::Header.into());
    }
    for _ in 0..6 {
        next(cursor, context, &mut crc)?;
    }
    if flags & 4 != 0 {
        let low = next(cursor, context, &mut crc)?;
        let high = next(cursor, context, &mut crc)?;
        for _ in 0..u16::from_le_bytes([low, high]) {
            next(cursor, context, &mut crc)?;
        }
    }
    for mask in [8, 16] {
        if flags & mask != 0 {
            while next(cursor, context, &mut crc)? != 0 {}
        }
    }
    if flags & 2 != 0 {
        context.budget.metadata()?;
        let low = cursor.byte(context, Stage::Header)?;
        context.budget.metadata()?;
        let high = cursor.byte(context, Stage::Header)?;
        if u16::from_le_bytes([low, high]) != crc as u16 {
            return Err(Failure::Header.into());
        }
    }
    Ok(cursor.position)
}

fn decode<E, F: FnMut(Checkpoint) -> Result<(), Stop<E>>>(
    decoder: &mut Inflate,
    cursor: &mut Cursor<'_>,
    context: &mut Context<'_, F>,
    header_bytes: usize,
    bytes: &mut Option<Vec<u8>>,
) -> Result<Boundary, Error<E>> {
    let mut output = [0; CHUNK_SIZE];
    let mut consumed_position = header_bytes;
    let mut offered_end = header_bytes;
    let mut crc = 0;
    loop {
        context.check(Stage::BeforeInflate)?;
        if consumed_position == offered_end && offered_end < cursor.input.len() {
            offered_end = offered_end
                .checked_add(CHUNK_SIZE)
                .ok_or(Failure::Boundary)?
                .min(cursor.input.len());
            cursor.account(offered_end, context.budget)?;
            cursor.position = offered_end;
        }
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let result = decoder.decompress(
            &cursor.input[consumed_position..offered_end],
            &mut output,
            InflateFlush::NoFlush,
        );
        // backend 오류보다 실제 출력 증가분 검증과 과금을 먼저 수행함.
        let produced = decoder
            .total_out()
            .checked_sub(before_out)
            .ok_or(Failure::Boundary)?;
        context.budget.decoded(produced)?;
        let status = result.map_err(Error::Backend)?;
        context.check(Stage::AfterInflate)?;
        let consumed = decoder
            .total_in()
            .checked_sub(before_in)
            .ok_or(Failure::Boundary)?;
        let consumed = usize::try_from(consumed).map_err(|_| Failure::Boundary)?;
        if consumed > offered_end - consumed_position {
            return Err(Failure::Boundary.into());
        }
        consumed_position = consumed_position
            .checked_add(consumed)
            .ok_or(Failure::Boundary)?;
        if decoder.total_in() != (consumed_position - header_bytes) as u64 {
            return Err(Failure::Boundary.into());
        }
        let produced = produced as usize;
        if produced != 0 {
            crc = crc32(crc, &output[..produced]);
            if let Some(saved) = bytes {
                context.budget.output(produced)?;
                saved
                    .try_reserve_exact(produced)
                    .map_err(|_| Failure::Allocation)?;
                saved.extend_from_slice(&output[..produced]);
                context.check(Stage::AfterOutput)?;
            }
        }
        if status == Status::StreamEnd {
            break;
        }
        if consumed == 0 && produced == 0 {
            return Err(if consumed_position == cursor.input.len() {
                Failure::Truncated
            } else {
                Failure::NoProgress
            }
            .into());
        }
    }
    context.check(Stage::Trailer)?;
    let read_ahead_bytes = offered_end
        .checked_sub(consumed_position)
        .ok_or(Failure::Boundary)?;
    cursor.position = consumed_position;
    let mut trailer = [0; 8];
    for byte in &mut trailer {
        *byte = cursor.byte(context, Stage::Trailer)?;
    }
    let expected_crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    let expected_size = u32::from_le_bytes([trailer[4], trailer[5], trailer[6], trailer[7]]);
    if expected_crc != crc {
        return Err(Failure::Crc.into());
    }
    if expected_size != decoder.total_out() as u32 {
        return Err(Failure::Size.into());
    }
    if cursor.position != cursor.input.len() {
        return Err(Failure::TrailingData.into());
    }
    context.check(Stage::Trailer)?;
    Ok(Boundary {
        header_bytes,
        deflate_bytes: decoder.total_in(),
        read_ahead_bytes,
        inflated_bytes: decoder.total_out(),
        crc32: crc,
        member_bytes: cursor.position,
        offered_bytes: (offered_end - header_bytes) as u64,
    })
}
