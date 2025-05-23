# Tiny Ogg stream manipulator

Ogg is a stream encapsulation format. It's not exclusively used for Vorbis or Opus audio files, but can encapsulate any type of continuous data streams for transmission protocols.

Personally, I consider Ogg to be an excellent binary data transmission protocol for UART/USART applications.

## Overview

### OggPacket
* The `OggPacket` represents a single data packet, the fundamental unit of an Ogg stream.
* Contains built-in capabilities for byte parsing and validation of complete packet structures.
* Provides checksum verification and regeneration functionality for raw packet bytes.
* Serves as a data container with capacity constraints (note: individual packets have size limitations).
* Supports data serialization to raw bytes and payload extraction.

The `OggPacket` have these functions:
```rust
fn new(stream_id: u32, packet_type: OggPacketType, packet_index: u32) -> Self;
fn write(&mut self, data: &[u8]) -> usize;
fn clear(&mut self);
fn get_segments(&self) -> Vec<Vec<u8>>;
fn get_inner_data_size(&self) -> usize;
fn get_inner_data(&self) -> Vec<u8>;
fn get_checksum(ogg_packet: &[u8]) -> io::Result<u32>;
fn fill_checksum_field(ogg_packet: &mut [u8]) -> io::Result<()>;
fn to_bytes(&self) -> Vec<u8>;
fn into_bytes(self) -> Vec<u8>;
fn get_length(ogg_packet: &[u8]) -> io::Result<usize>;
fn from_bytes(ogg_packet: &[u8], packet_length: &mut usize) -> io::Result<Self>;
fn from_cursor(cursor: &mut Cursor<Vec<u8>>) -> Vec<OggPacket>;
```

### OggStreamReader
* `OggStreamReader<R: Read + Debug>` provides sequential access to Ogg streams.
* Initialize with any `Read` implementer (e.g., `File`, `BufReader`, `Cursor`)
* Continuously call `get_packet()` to retrieve packets from all streams in the source.
* Return values:
	* `Ok(Some(packet))`: Valid packet retrieved
	* `Ok(None)`: End of input reached
	* `Err(io::Error)`: Error occurred

The `OggStreamReader` have these functions:
```rust
fn new(reader: R) -> Self;
fn get_packet(&mut self) -> io::Result<Option<OggPacket>>;
fn is_eos(&self) -> bool;
fn is_eof(&self) -> bool;
```

### OggStreamWriter
* `OggStreamWriter<W: Write + Debug>` handles Ogg stream output
* Initialize with any `Write` implementer (e.g., `File`, `BufWriter`, `Cursor`)
* Implements `Write` trait with automatic packet management:
	* Buffers data into packets
	* Auto-seals and flushes full packets
	* Customizable granule position calculation via `on_seal` callback
* Manual packet sealing via `seal_packet()`

The `OggStreamWriter` have these functions:
```rust
fn new(writer: W, stream_id: u32) -> Self;
fn set_granule_position(&mut self, position: u64);
fn get_granule_position(&self) -> u64;
fn mark_cur_packet_as_end_of_stream(&mut self);
fn get_bytes_written(&self) -> u64;
fn set_on_seal_callback(&mut self, on_seal: Box<dyn FnMut(usize) -> u64>);
fn reset(&mut self);
fn seal_packet(&mut self, granule_position: u64, is_end_of_stream: bool) -> io::Result<()>;
```

## For more information about each function please read the documentations.

```rust
#[derive(Debug, Clone, Copy)]
pub enum OggPacketType {
	/// * The middle packets
	Continuation = 0,

	/// * The begin of a stream
	BeginOfStream = 2,

	/// * The last packet of a stream
	EndOfStream = 4,
}

/// * An ogg packet as a stream container
#[derive(Debug, Clone)]
pub struct OggPacket {
	/// Ogg Version must be zero
	pub version: u8,

	/// * The first packet should be `OggPacketType::BeginOfStream`
	/// * The last packet should be `OggPacketType::EndOfStream`
	/// * The others should be `OggPacketType::Continuation`
	pub packet_type: OggPacketType,

	/// * For vorbis, this field indicates when you had decoded from the first packet to this packet,
	///   and you had finished decoding this packet, how many of the audio frames you should get.
	pub granule_position: u64,

	/// * The identifier for the streams. Every Ogg packet belonging to a stream should have the same `stream_id`.
	pub stream_id: u32,

	/// * The index of the packet, beginning from zero.
	pub packet_index: u32,

	/// * The checksum of the packet.
	pub checksum: u32,

	/// * A table indicates each segment's size, the max is 255. And the size of the table also couldn't exceed 255.
	pub segment_table: Vec<u8>,

	/// * The data encapsulated in the Ogg Stream
	pub data: Vec<u8>,
}

/// * An ogg packet reader
pub struct OggStreamReader<R>
where
	R: Read + Debug {
	/// * The reader
	pub reader: R,

	/// * The unique stream ID, after read out the first packet, this field is set.
	pub stream_id: u32,

	/// * If an EOS is encountered, this field is set to true
	e_o_s: bool,

	/// * If encountered EOF, this field is set to true
	e_o_f: bool,

	/// * The cached bytes for next read
	cached_bytes: Vec<u8>,
}

/// * An ogg packet as a stream container
pub struct OggStreamWriter<W>
where
	W: Write + Debug {
	/// * The writer, when a packet is full or you want to seal the packet, the packet is flushed in the writer
	pub writer: W,

	/// * The unique stream ID for a whole stream. Programs use the stream ID to identify which packet is for which stream.
	pub stream_id: u32,

	/// * The packet index.
	pub packet_index: u32,

	/// * The current packet, ready to be written.
	pub cur_packet: OggPacket,

	/// * The granule position is for the programmers to reference it for some purpose.
	pub granule_position: u64,

	/// * The `OggStreamWriter<W>` implements `Write`, when the `cur_packet` is full, the `on_seal()` closure will be called for updating the granule position.
	/// * And then the packet will be flushed into the writer.
	pub on_seal: Box<dyn FnMut(usize) -> u64>,

	/// * How many bytes were written into this stream.
	pub bytes_written: u64,
}
```

