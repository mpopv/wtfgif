export enum GIF {
	// Header
	G = 0x47,
	I = 0x49,
	F = 0x46,
	_7 = 0x37,
	_8 = 0x38,
	_9 = 0x39,
	A = 0x61,
	// Blocks
	EXT = 0x21,
	IMG = 0x2c,
	TRAILER = 0x3b,
	// Extension labels
	GCE = 0xf9,
	APPLICATION = 0xff,
	PLAINTEXT = 0x01,
	COMMENT = 0xfe,
	// NETSCAPE2.0
	NETSCAPE_LEN = 0x0b,
	// Limits
	MAX_CODE = 4096,
}
