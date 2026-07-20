#[cfg(test)]
mod tests {
    use crate::builder::OHLK_Builder;
    use crate::crc32_ieee;
    use crate::entry::SegmentType;
    use crate::parser::OHLK_Parser;

    #[test]
    fn test_crc32() {
        let data = b"123456789";
        // Standard CRC32-IEEE value for "123456789" is 0xCBF43926 (3421780262)
        let calc = crc32_ieee(data);
        assert_eq!(calc, 0xCBF43926);
    }

    #[test]
    fn test_builder_and_parser() {
        let mut builder = OHLK_Builder::new(1, 0); // ARM64, flags=0

        let text_data = [0xAA, 0xBB, 0xCC, 0xDD];
        let data_data = [0x11, 0x22, 0x33, 0x44, 0x55];

        let text_idx = builder.add_segment(SegmentType::Text.to_u32(), 5, &text_data, 4); // R+X
        let data_idx = builder.add_segment(SegmentType::Data.to_u32(), 3, &data_data, 5); // R+W

        assert_eq!(text_idx, 0);
        assert_eq!(data_idx, 1);

        let binary = builder.build().expect("Build should succeed");

        // Parse and verify
        let parser = OHLK_Parser::new(&binary).expect("Parse should succeed");
        let header = parser.header();

        assert_eq!(header.magic, 0x4F484C4B);
        assert_eq!(header.arch, 1);
        assert_eq!(header.header_count, 2);

        let entry_0 = parser.get_entry(0).expect("Entry 0 should exist");
        assert_eq!(entry_0.ty, SegmentType::Text.to_u32());
        assert_eq!(entry_0.flags, 5);
        assert_eq!(entry_0.file_size, 4);

        let segment_0_data = parser
            .get_segment_data(&entry_0)
            .expect("Segment 0 data should be readable");
        assert_eq!(segment_0_data, &text_data);

        let entry_1 = parser.get_entry(1).expect("Entry 1 should exist");
        assert_eq!(entry_1.ty, SegmentType::Data.to_u32());
        assert_eq!(entry_1.flags, 3);
        assert_eq!(entry_1.file_size, 5);

        let segment_1_data = parser
            .get_segment_data(&entry_1)
            .expect("Segment 1 data should be readable");
        assert_eq!(segment_1_data, &data_data);
    }
}
