#![allow(dead_code)]

use std::{
	cmp::max,
	io::{self, Read, Write, Cursor, ErrorKind},
	mem,
	fmt::{self, Debug, Formatter}
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OggPacketType {
	/// * The middle packets
	Continuation = 0,

	/// * The begin of a stream
	BeginOfStream = 2,

	/// * The last packet of a stream
	EndOfStream = 4,
}

/// * An ogg packet as a stream container
#[derive(Clone)]
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

impl OggPacket {
	/// Create a new Ogg packet
	pub fn new(stream_id: u32, packet_type: OggPacketType, packet_index: u32) -> Self {
		Self {
			version: 0,
			packet_type,
			granule_position: 0,
			stream_id,
			packet_index,
			checksum: 0,
			segment_table: Vec::new(),
			data: Vec::new(),
		}
	}

	/// Write some data to the packet, returns the actual written bytes.
	pub fn write(&mut self, data: &[u8]) -> usize {
		let mut written = 0usize;
		let mut to_write = data.len();
		if to_write == 0 {
			return 0;
		}
		while self.segment_table.len() < 255 {
			if to_write >= 255 {
				let new_pos = written + 255;
				self.segment_table.push(255);
				self.data.extend(data[written..new_pos].to_vec());
				written = new_pos;
				to_write -= 255;
			} else {
				if to_write == 0 {
					break;
				}
				let new_pos = written + to_write;
				self.segment_table.push(to_write as u8);
				self.data.extend(data[written..new_pos].to_vec());
				written = new_pos;
				break;
			}
		}
		written
	}

	/// Clear all data inside the packet
	pub fn clear(&mut self) {
		self.segment_table = Vec::new();
		self.data = Vec::new();
	}

	/// Read all of the data as segments from the packet
	pub fn get_segments(&self) -> Vec<Vec<u8>> {
		let mut ret = Vec::<Vec<u8>>::with_capacity(self.segment_table.len());
		let mut pos = 0usize;
		self.segment_table.iter().for_each(|&size|{
			let next_pos = pos + size as usize;
			ret.push(self.data[pos..next_pos].to_vec());
			pos = next_pos;
		});
		ret
	}

	/// Get inner data size
	pub fn get_inner_data_size(&self) -> usize {
		self.segment_table.iter().map(|&s|s as usize).sum()
	}

	/// Read all of the data as a flattened `Vec<u8>`
	pub fn get_inner_data(&self) -> Vec<u8> {
		self.get_segments().into_iter().flatten().collect()
	}

	/// Read all of the data as a flattened `Vec<u8>` and consume self
	pub fn into_inner(self) -> Vec<u8> {
		self.get_inner_data()
	}

	/// Calculate the checksum
	pub fn crc(mut crc: u32, data: &[u8]) -> u32 {
        type CrcTableType = [u32; 256];
        fn ogg_generate_crc_table() -> CrcTableType {
            use std::mem::MaybeUninit;
            #[allow(invalid_value)]
            #[allow(clippy::uninit_assumed_init)]
            let mut crc_lookup: CrcTableType = unsafe{MaybeUninit::uninit().assume_init()};
            (0..256).for_each(|i|{
                let mut r: u32 = i << 24;
                for _ in 0..8 {
                    r = (r << 1) ^ (-(((r >> 31) & 1) as i32) as u32 & 0x04c11db7);
                }
                crc_lookup[i as usize] = r;
            });
            crc_lookup
        }

        use std::sync::OnceLock;
        static OGG_CRC_TABLE: OnceLock<CrcTableType> = OnceLock::<CrcTableType>::new();
        let crc_lookup = OGG_CRC_TABLE.get_or_init(ogg_generate_crc_table);

        for b in data {
            crc = (crc << 8) ^ crc_lookup[(*b as u32 ^ (crc >> 24)) as usize];
        }

        crc
	}

	pub fn get_checksum(ogg_packet: &[u8]) -> io::Result<u32> {
		if ogg_packet.len() < 27 {
			Err(io::Error::new(ErrorKind::InvalidData, format!("The given packet is too small: {} < 27", ogg_packet.len())))
		} else {
			let mut field_cleared = ogg_packet.to_vec();
			field_cleared[22..26].copy_from_slice(&[0u8; 4]);
			Ok(Self::crc(0, &field_cleared))
		}
	}

	/// Set the checksum for the Ogg packet
	pub fn fill_checksum_field(ogg_packet: &mut [u8]) -> io::Result<()> {
		let checksum = Self::get_checksum(ogg_packet)?;
		ogg_packet[22..26].copy_from_slice(&checksum.to_le_bytes());
		Ok(())
	}

	/// Serialize the packet to bytes. Only in the bytes form can calculate the checksum.
	pub fn into_bytes(self) -> Vec<u8> {
		let mut ret: Vec<u8> = [
			b"OggS" as &[u8],
			&[self.version],
			&[self.packet_type as u8],
			&self.granule_position.to_le_bytes() as &[u8],
			&self.stream_id.to_le_bytes() as &[u8],
			&self.packet_index.to_le_bytes() as &[u8],
			&0u32.to_le_bytes() as &[u8],
			&[self.segment_table.len() as u8],
			&self.segment_table,
			&self.data,
		].into_iter().flatten().copied().collect();
		Self::fill_checksum_field(&mut ret).unwrap();
		ret
	}

	/// Retrieve the packet length in bytes
	pub fn get_length(ogg_packet: &[u8]) -> io::Result<usize> {
		if ogg_packet.len() < 27 {
			Err(io::Error::new(ErrorKind::UnexpectedEof, format!("The given ogg page size is too small: {} < 27", ogg_packet.len())))
		} else if ogg_packet[0..4] != *b"OggS" {
			Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: expected `OggS`, got `{}`", String::from_utf8_lossy(&ogg_packet[0..4]))))
		} else if ogg_packet[4] != 0 {
			Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: invalid `version` = {} (should be zero)", ogg_packet[4])))
		} else {
			match ogg_packet[5] {
				0 | 2 | 4 => (),
				o => return Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: invalid `packet_type` = {o} (should be 0, 2, 4)"))),
			}
			let num_segments = ogg_packet[26] as usize;
			let data_start = 27 + num_segments;
			let segment_table = &ogg_packet[27..data_start];
			let data_length: usize = segment_table.iter().map(|&s|s as usize).sum();
			Ok(data_start + data_length)
		}
	}

	/// Deserialize the packet
	pub fn from_bytes(ogg_packet: &[u8], packet_length: &mut usize) -> io::Result<Self> {
		if ogg_packet.len() < 27 {
			Err(io::Error::new(ErrorKind::UnexpectedEof, format!("The given data size is too small: {} < 27", ogg_packet.len())))
		} else if ogg_packet[0..4] != *b"OggS" {
			Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: expected `OggS`, got `{}`", String::from_utf8_lossy(&ogg_packet[0..4]))))
		} else if ogg_packet[4] != 0 {
			Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: invalid `version` = {} (should be zero)", ogg_packet[4])))
		} else {
			let packet_type = match ogg_packet[5] {
				0 => OggPacketType::Continuation,
				2 => OggPacketType::BeginOfStream,
				4 => OggPacketType::EndOfStream,
				o => return Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: invalid `packet_type` = {o} (should be 0, 2, 4)"))),
			};
			let num_segments = ogg_packet[26] as usize;
			let data_start = 27 + num_segments;
			if data_start > ogg_packet.len() {
				return Err(io::Error::new(ErrorKind::UnexpectedEof, format!("The given data size is too small: {}", ogg_packet.len())));
			}
			let segment_table = &ogg_packet[27..data_start];
			let data_length: usize = segment_table.iter().map(|&s|s as usize).sum();
			*packet_length = data_start + data_length;
			if ogg_packet.len() < *packet_length {
				Err(io::Error::new(ErrorKind::UnexpectedEof, format!("The given data size is too small: {} < {packet_length}", ogg_packet.len())))
			} else {
				let ret = Self{
					version: 0,
					packet_type,
					granule_position: u64::from_le_bytes(ogg_packet[6..14].try_into().unwrap()),
					stream_id: u32::from_le_bytes(ogg_packet[14..18].try_into().unwrap()),
					packet_index: u32::from_le_bytes(ogg_packet[18..22].try_into().unwrap()),
					checksum: u32::from_le_bytes(ogg_packet[22..26].try_into().unwrap()),
					segment_table: segment_table.to_vec(),
					data: ogg_packet[data_start..*packet_length].to_vec(),
				};
				let checksum = Self::get_checksum(&ogg_packet[..*packet_length])?;
				if ret.checksum != checksum {
					Err(io::Error::new(ErrorKind::InvalidData, format!("Ogg packet checksum not match: should be 0x{:x}, got 0x{:x}", checksum, ret.checksum)))
				} else {
					Ok(ret)
				}
			}
		}
	}

	/// Deserialize to multiple packets
	pub fn from_cursor(cursor: &mut Cursor<Vec<u8>>) -> Vec<OggPacket> {
		let mut data: &[u8] = cursor.get_ref();
		let mut packet_length = 0usize;
		let mut bytes_read = 0usize;
		let mut ret = Vec::<OggPacket>::new();
		while let Ok(packet) = Self::from_bytes(data, &mut packet_length) {
			bytes_read += packet_length;
			ret.push(packet);
			data = &data[packet_length..];
			if data.is_empty() {
				break;
			}
		}
		cursor.set_position(bytes_read as u64);
		ret
	}
}

impl Debug for OggPacket {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		f.debug_struct("OggPacket")
		.field("version", &self.version)
		.field("packet_type", &self.packet_type)
		.field("granule_position", &self.granule_position)
		.field("stream_id", &self.stream_id)
		.field("packet_index", &self.packet_index)
		.field("checksum", &format_args!("0x{:08x}", self.checksum))
		.field("segment_table", &self.segment_table)
		.field("data", &format_args!("[u8; {}]", self.data.len()))
		.finish()
	}
}

impl Default for OggPacket {
	fn default() -> Self {
		Self {
			version: 0,
			packet_type: OggPacketType::BeginOfStream,
			granule_position: 0,
			stream_id: 0,
			packet_index: 0,
			checksum: 0,
			segment_table: Vec::new(),
			data: Vec::new(),
		}
	}
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

impl<R> OggStreamReader<R>
where
	R: Read + Debug {
	const READ_SIZE: usize = 2048;

	pub fn new(reader: R) -> Self {
		Self {
			reader,
			stream_id: 0,
			e_o_s: false,
			e_o_f: false,
			cached_bytes: Vec::new(),
		}
	}

	fn safe_read(&mut self, target_len: usize) -> io::Result<Vec<u8>> {
		let mut buf = vec![0u8; target_len];
		let mut bytes_read = 0usize;
		while bytes_read < target_len {
			let read = match self.reader.read(&mut buf[bytes_read..]) {
				Ok(0) => break,
				Ok(size) => size,
				Err(e) => match e.kind() {
					io::ErrorKind::Interrupted => {
						0
					}
					io::ErrorKind::UnexpectedEof => {
						break;
					}
					_ => {
						if bytes_read > 0 {
							break;
						} else {
							return Err(e);
						}
					}
				}
			};
			bytes_read += read;
		}
		buf.truncate(bytes_read);
		Ok(buf)
	}

	pub fn get_packet(&mut self) -> io::Result<Option<OggPacket>> {
		let mut packet_length = 0usize;
		match OggPacket::from_bytes(&self.cached_bytes, &mut packet_length) {
			Ok(packet) => {
				if packet.packet_type == OggPacketType::EndOfStream {
					self.e_o_s = true;
				} else {
					self.e_o_s = false;
				}
				self.cached_bytes = self.cached_bytes[packet_length..].to_vec();
				Ok(Some(packet))
			}
			Err(e) => match e.kind() {
				io::ErrorKind::UnexpectedEof => { // Not enough bytes for an Ogg packet
					if self.e_o_s {
						Ok(None)
					} else {
						let to_read = max(packet_length, Self::READ_SIZE);
						let read = self.safe_read(to_read)?;
						self.cached_bytes.extend(&read);
						if read.len() < to_read {
							if self.e_o_f == false {
								self.e_o_f = true;
								self.get_packet()
							} else {
								if read.len() == 0 {
									Ok(None)
								} else {
									Err(e)
								}
							}
						} else {
							self.get_packet()
						}
					}
				}
				_ => Err(e)
			}
		}
	}

	pub fn is_eos(&self) -> bool {
		self.e_o_s
	}

	pub fn is_eof(&self) -> bool {
		self.e_o_f
	}
}


/// * An ogg packets writer sink
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

impl<W> OggStreamWriter<W>
where
	W: Write + Debug {
	pub fn new(writer: W, stream_id: u32) -> Self {
		Self {
			writer,
			stream_id,
			packet_index : 0,
			cur_packet: OggPacket::new(stream_id, OggPacketType::BeginOfStream, 0),
			granule_position: 0,
			bytes_written: 0,
			on_seal: Box::new(|i|i as u64),
		}
	}

	/// * Set the granule position. This field of data is not used by the Ogg stream.
	/// * The granule position is for the inner things to reference it for some purpose.
	pub fn set_granule_position(&mut self, position: u64) {
		self.granule_position = position
	}

	/// * Get the granule position you had set before
	pub fn get_granule_position(&self) -> u64 {
		self.granule_position
	}

	/// * Mark the current packet as EOS
	pub fn mark_cur_packet_as_end_of_stream(&mut self) {
		self.cur_packet.packet_type = OggPacketType::EndOfStream;
	}

	/// * Get how many bytes written in this stream
	pub fn get_bytes_written(&self) -> u64 {
		self.bytes_written
	}

	/// * Set a callback for the `Write` trait when it seals the packet, the callback helps with updating the granule position
	pub fn set_on_seal_callback(&mut self, on_seal: Box<dyn FnMut(usize) -> u64>) {
		self.on_seal = on_seal;
	}

	/// * Reset the stream state, discard the packet, reinit the packet to a BOS
	pub fn reset(&mut self) {
		self.packet_index = 0;
		self.cur_packet = OggPacket::new(self.stream_id, OggPacketType::BeginOfStream, 0);
		self.granule_position = 0;
		self.bytes_written = 0;
	}

	/// * Save the current packet and write it to the sink, then create a new packet for writing.
	pub fn seal_packet(&mut self, granule_position: u64, is_end_of_stream: bool) -> io::Result<()> {
		self.packet_index += 1;
		self.granule_position = granule_position;
		self.cur_packet.granule_position = self.granule_position;
		let packed = if is_end_of_stream {
			self.cur_packet.packet_type = OggPacketType::EndOfStream;
			mem::take(&mut self.cur_packet).into_bytes()
		} else {
			mem::replace(&mut self.cur_packet, OggPacket::new(self.stream_id, OggPacketType::Continuation, self.packet_index)).into_bytes()
		};
		self.writer.write_all(&packed)?;
		Ok(())
	}
}

impl<W> Write for OggStreamWriter<W>
where
	W: Write + Debug {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		self.bytes_written = buf.len() as u64;
		let mut buf = buf;
		let mut written_total = 0usize;
		while !buf.is_empty() {
			let written = self.cur_packet.write(buf);
			buf = &buf[written..];
			written_total += written;
			if !buf.is_empty() {
				self.granule_position = (self.on_seal)(self.cur_packet.get_inner_data_size());
				self.seal_packet(self.granule_position, false)?;
			}
		}
		Ok(written_total)
	}

	fn flush(&mut self) -> io::Result<()> {
		self.writer.flush()
	}
}

impl<W> Debug for OggStreamWriter<W>
where
	W: Write + Debug {
	fn fmt(&self, f: &mut Formatter) -> fmt::Result {
		f.debug_struct(&format!("OggStreamWriter<{}>", std::any::type_name::<W>()))
		.field("writer", &self.writer)
		.field("stream_id", &format_args!("0x{:08x}", self.stream_id))
		.field("packet_index", &self.packet_index)
		.field("cur_packet", &self.cur_packet)
		.field("granule_position", &self.granule_position)
		.field("on_seal", &format_args!("<closure>"))
		.field("bytes_written", &self.bytes_written)
		.finish()
	}
}

impl<W> Drop for OggStreamWriter<W>
where
	W: Write + Debug {
	fn drop(&mut self) {
		self.seal_packet(self.granule_position, true).unwrap();
	}
}

#[test]
fn test_ogg() {
	use std::{
		fs::File,
		io::BufReader,
	};
	let mut oggreader = OggStreamReader::new(BufReader::new(File::open("test.ogg").unwrap()));
	loop {
		let packet = oggreader.get_packet().unwrap();
		if let Some(packet) = packet {
			dbg!(packet);
		} else {
			break;
		}
	}
}
