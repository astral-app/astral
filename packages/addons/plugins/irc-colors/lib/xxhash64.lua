local PRIME_1 = -7046029288634856825
local PRIME_2 = -4417276706812531889
local PRIME_3 = 1609587929392839161
local PRIME_4 = -8796714831421723037
local PRIME_5 = 2870177450012600261

local function rotate(value, bits)
	return (value << bits) | (value >> (64 - bits))
end

local function round(accumulator, input)
	return rotate(accumulator + input * PRIME_2, 31) * PRIME_1
end

local function merge(accumulator, value)
	return (accumulator ~ round(0, value)) * PRIME_1 + PRIME_4
end

local function stripes(text, seed)
	local length = #text
	if length < 32 then
		return seed + PRIME_5, 1
	end
	local lanes = { seed + PRIME_1 + PRIME_2, seed + PRIME_2, seed, seed - PRIME_1 }
	local offset = 1
	while offset + 31 <= length do
		for lane = 1, 4 do
			lanes[lane] = round(lanes[lane], string.unpack("<i8", text, offset + (lane - 1) * 8))
		end
		offset = offset + 32
	end
	local hash = rotate(lanes[1], 1) + rotate(lanes[2], 7) + rotate(lanes[3], 12) + rotate(lanes[4], 18)
	for lane = 1, 4 do
		hash = merge(hash, lanes[lane])
	end
	return hash, offset
end

local function hash(text, seed)
	local length = #text
	local value, offset = stripes(text, seed or 0)
	value = value + length
	while offset + 7 <= length do
		value = value ~ round(0, string.unpack("<i8", text, offset))
		value = rotate(value, 27) * PRIME_1 + PRIME_4
		offset = offset + 8
	end
	if offset + 3 <= length then
		value = value ~ (string.unpack("<I4", text, offset) * PRIME_1)
		value = rotate(value, 23) * PRIME_2 + PRIME_3
		offset = offset + 4
	end
	while offset <= length do
		value = value ~ (string.byte(text, offset) * PRIME_5)
		value = rotate(value, 11) * PRIME_1
		offset = offset + 1
	end
	value = value ~ (value >> 33)
	value = value * PRIME_2
	value = value ~ (value >> 29)
	value = value * PRIME_3
	return value ~ (value >> 32)
end

local function unsigned_modulo(value, divisor)
	return ((value >> 1) % (divisor // 2) * 2 + (value & 1)) % divisor
end

return { hash = hash, unsigned_modulo = unsigned_modulo }
