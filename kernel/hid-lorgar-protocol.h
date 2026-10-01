#ifndef HID_LORGAR_PROTOCOL_H
#define HID_LORGAR_PROTOCOL_H

#ifdef __KERNEL__
#include <linux/types.h>
#define LORGAR_U8 u8
#else
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#define LORGAR_U8 uint8_t
#endif

#define LORGAR_STATE_REPORT_LEN 64
#define LORGAR_DPI_STAGE_COUNT 8
#define LORGAR_EVENT_REPORT_ID 0x05
#define LORGAR_EVENT_DPI_TOGGLE 0x0b

static inline bool lorgar_parse_state_report(const LORGAR_U8 *data, size_t size,
					     LORGAR_U8 *capacity)
{
	if (!data || !capacity || size != LORGAR_STATE_REPORT_LEN ||
	    (data[0] != 0x01 && data[0] != 0x02) || data[1] != 0x01 ||
	    data[7] > 100)
		return false;
	*capacity = data[7];
	return true;
}

static inline bool lorgar_parse_dpi_event(LORGAR_U8 report_id,
					  const LORGAR_U8 *data, size_t size,
					  LORGAR_U8 *stage)
{
	if (!data || !stage || report_id != LORGAR_EVENT_REPORT_ID || size < 3 ||
	    data[0] != LORGAR_EVENT_REPORT_ID ||
	    data[1] != LORGAR_EVENT_DPI_TOGGLE ||
	    data[2] >= LORGAR_DPI_STAGE_COUNT)
		return false;
	*stage = data[2];
	return true;
}

#undef LORGAR_U8

#endif
