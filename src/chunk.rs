use crate::Error3ds;

/// A parsed 3DS chunk header + data bounds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Chunk {
    /// Chunk type identifier.
    pub id: u16,
    /// Absolute byte offset where the chunk header starts.
    pub offset: usize,
    /// Absolute byte offset where the chunk's *data* starts (after the 6-byte header).
    pub data_start: usize,
    /// Absolute byte offset one past the end of this chunk.
    pub end: usize,
}

impl Chunk {
    /// Read a chunk header at the given offset.
    ///
    /// # Errors
    /// Returns [`Error3ds::Truncated`] if there are fewer than 6 bytes remaining.
    pub fn read_at(data: &[u8], offset: usize) -> Result<Self, Error3ds> {
        if data.len() < offset + 6 {
            return Err(Error3ds::Truncated(data.len()));
        }

        let id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        let length = u32::from_le_bytes([
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
        ]);

        if length < 6 {
            return Err(Error3ds::ChunkOverflow { id, offset, length });
        }

        let end = offset + length as usize;

        Ok(Chunk {
            id,
            offset,
            data_start: offset + 6,
            end,
        })
    }
}

/// Iterator over the immediate child chunks of `parent`.
pub(crate) struct ChunkIter<'a> {
    data: &'a [u8],
    cursor: usize,
    end: usize,
}

impl<'a> Iterator for ChunkIter<'a> {
    type Item = Result<Chunk, Error3ds>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.cursor >= self.end {
            return None;
        }

        let chunk = match Chunk::read_at(self.data, self.cursor) {
            Ok(c) => c,
            Err(e) => return Some(Err(e)),
        };

        if chunk.end > self.end || chunk.end > self.data.len() {
            return Some(Err(Error3ds::ChunkOverflow {
                id: chunk.id,
                offset: chunk.offset,
                length: (chunk.end - chunk.offset) as u32,
            }));
        }

        self.cursor = chunk.end;
        Some(Ok(chunk))
    }
}

/// Iterate over children of `parent`, starting from `parent.data_start`.
pub(crate) fn walk_chunks<'a>(
    data: &'a [u8],
    parent: &Chunk,
) -> Result<ChunkIter<'a>, Error3ds> {
    walk_chunks_from(data, parent, parent.data_start)
}

/// Iterate over children of `parent`, starting from a custom offset.
pub(crate) fn walk_chunks_from<'a>(
    data: &'a [u8],
    parent: &Chunk,
    start: usize,
) -> Result<ChunkIter<'a>, Error3ds> {
    if start > parent.end {
        return Err(Error3ds::BadOffset {
            id: parent.id,
            start,
            end: parent.end,
        });
    }

    Ok(ChunkIter {
        data,
        cursor: start,
        end: parent.end,
    })
}
