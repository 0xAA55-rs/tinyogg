#![allow(dead_code)]

use std::{io::{self, Cursor, Write, ErrorKind}, mem};

#[derive(Debug, Clone, Copy)]
pub enum OggPacketType {
	Continuation = 0,
	BeginOfStream = 2,
	EndOfStream = 4,
}

/// ## An ogg packet as a stream container
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
		return written;
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

	/// Read all of the data as a flattened `Vec<u8>`
	pub fn get_inner_data(&self) -> Vec<u8> {
		self.get_segments().into_iter().flatten().collect()
	}

	/// Calculate the checksum
	pub fn crc(mut crc: u32, data: &[u8]) -> u32 {
        type CrcTableType = [u32; 256];
        fn ogg_generate_crc_table() -> CrcTableType {
            use std::mem::MaybeUninit;
            #[allow(invalid_value)]
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
        let crc_lookup = OGG_CRC_TABLE.get_or_init(|| ogg_generate_crc_table());

        for b in data {
            crc = (crc << 8) ^ crc_lookup[(*b as u32 ^ (crc >> 24)) as usize];
        }

        crc
	}

	pub fn get_checksum(ogg_packet: &[u8]) -> Result<u32, io::Error> {
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
		Ok(ogg_packet[22..26].copy_from_slice(&checksum.to_le_bytes()))
	}

	/// Serialize the packet to bytes. Only in the bytes form can calculate the checksum.
	pub fn to_bytes(self) -> Vec<u8> {
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
			Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: expected `OggS`, got `{}`", String::from_utf8_lossy(&ogg_packet[0..4]).to_string())))
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
	pub fn from_bytes(ogg_packet: &[u8], packet_length: &mut usize) -> Result<Self, io::Error> {
		if ogg_packet.len() < 27 {
			Err(io::Error::new(ErrorKind::UnexpectedEof, format!("The given ogg page size is too small: {} < 27", ogg_packet.len())))
		} else if ogg_packet[0..4] != *b"OggS" {
			Err(io::Error::new(ErrorKind::InvalidData, format!("While parsing Ogg packet: expected `OggS`, got `{}`", String::from_utf8_lossy(&ogg_packet[0..4]).to_string())))
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
			let segment_table = &ogg_packet[27..data_start];
			let data_length: usize = segment_table.iter().map(|&s|s as usize).sum();
			*packet_length = data_start + data_length;
			if ogg_packet.len() < *packet_length {
				Err(io::Error::new(ErrorKind::UnexpectedEof, format!("The given ogg page size is too small: {} < {packet_length}", ogg_packet.len())))
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
		let mut data: &[u8] = &*cursor.get_ref();
		let mut packet_length = 0usize;
		let mut bytes_read = 0usize;
		let mut ret = Vec::<OggPacket>::new();
		loop {
			match Self::from_bytes(&data, &mut packet_length) {
				Ok(packet) => {
					bytes_read += packet_length;
					ret.push(packet);
					data = &data[packet_length..];
					if data.is_empty() {
						break;
					}
				}
				Err(_) => break,
			}
		}
		cursor.set_position(bytes_read as u64);
		ret
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

/// ## An ogg packet as a stream container
#[derive(Debug, Clone)]
pub struct OggStreamWriter<W>
where
	W: Write {
	pub writer: W,
	pub stream_id: u32,
	pub packet_index: u32,
	pub cur_packet: OggPacket,
	pub granule_position: u64,
}

impl<W> OggStreamWriter<W>
where
	W: Write {
	pub fn new(writer: W, stream_id: u32) -> Self {
		Self {
			writer,
			stream_id,
			packet_index : 0,
			cur_packet: OggPacket::new(stream_id, OggPacketType::BeginOfStream, 0),
			granule_position: 0,
		}
	}

	pub fn set_granule_position(&mut self, position: u64) {
		self.granule_position = position
	}

	pub fn get_granule_position(&self) -> u64 {
		self.granule_position
	}

	pub fn set_to_end_of_stream(&mut self) {
		self.cur_packet.packet_type = OggPacketType::EndOfStream;
	}

	pub fn reset(&mut self) {
		self.packet_index = 0;
		self.cur_packet = OggPacket::new(self.stream_id, OggPacketType::BeginOfStream, 0);
		self.granule_position = 0;
	}

	pub fn seal_packet(&mut self, granule_position: u64, is_end_of_stream: bool) -> io::Result<()> {
		self.packet_index += 1;
		self.granule_position = granule_position;
		self.cur_packet.granule_position = self.granule_position;
		let packed = if is_end_of_stream {
			self.cur_packet.packet_type = OggPacketType::EndOfStream;
			mem::take(&mut self.cur_packet).to_bytes()
		} else {
			mem::replace(&mut self.cur_packet, OggPacket::new(self.stream_id, OggPacketType::Continuation, self.packet_index)).to_bytes()
		};
		self.writer.write_all(&packed)?;
		Ok(())
	}
}

impl<W> Write for OggStreamWriter<W>
where
	W: Write {
	fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
		let mut buf = buf;
		let mut written_total = 0usize;
		while !buf.is_empty() {
			let written = self.cur_packet.write(buf);
			buf = &buf[written..];
			written_total += written;
			if buf.len() > 0 {
				self.seal_packet(self.granule_position, false)?;
			}
		}
		Ok(written_total)
	}

	fn flush(&mut self) -> io::Result<()> {
		self.writer.flush()
	}
}

impl<W> Drop for OggStreamWriter<W>
where
	W: Write {
	fn drop(&mut self) {
		self.seal_packet(self.granule_position, true).unwrap();
	}
}

