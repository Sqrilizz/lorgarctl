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
#define LORGAR_FEATURE_REPORT_LEN 8
#define LORGAR_DPI_STAGE_COUNT 8
#define LORGAR_EVENT_REPORT_ID 0x05
#define LORGAR_EVENT_DPI_TOGGLE 0x0b
#define LORGAR_CMD_STAGE_COLOR 0x27
#define LORGAR_CMD_ACTIVE_STAGE 0x21

static inline LORGAR_U8 lorgar_battery_indicator_band(LORGAR_U8 capacity)
{
	if (capacity <= 20)
		return 0;
	if (capacity <= 50)
		return 1;
	if (capacity <= 80)
		return 2;
	return 3;
}

static inline void lorgar_battery_indicator_color(LORGAR_U8 capacity,
					   LORGAR_U8 *red,
					   LORGAR_U8 *green,
					   LORGAR_U8 *blue)
{
	*blue = 0;
	switch (lorgar_battery_indicator_band(capacity)) {
	case 0:
		*red = 0xff;
		*green = 0;
		break;
	case 1:
		*red = 0xff;
		*green = 0x8c;
		break;
	case 2:
		*red = 0xff;
		*green = 0xd0;
		break;
	default:
		*red = 0x32;
		*green = 0xcd;
		break;
	}
}

static inline void lorgar_build_stage_color_report(LORGAR_U8 *report,
						    LORGAR_U8 stage,
						    LORGAR_U8 capacity)
{
	LORGAR_U8 red;
	LORGAR_U8 green;
	LORGAR_U8 blue;
	size_t i;

	for (i = 0; i < LORGAR_FEATURE_REPORT_LEN; i++)
		report[i] = 0;
	lorgar_battery_indicator_color(capacity, &red, &green, &blue);
	report[0] = LORGAR_CMD_STAGE_COLOR;
	report[2] = stage;
	report[3] = red;
	report[4] = green;
	report[5] = blue;
}

static inline void lorgar_build_active_stage_report(LORGAR_U8 *report,
						     LORGAR_U8 stage)
{
	size_t i;

	for (i = 0; i < LORGAR_FEATURE_REPORT_LEN; i++)
		report[i] = 0;
	report[0] = LORGAR_CMD_ACTIVE_STAGE;
	report[3] = stage;
}

static inline bool lorgar_parse_state_report(const LORGAR_U8 *data, size_t size,
					     LORGAR_U8 *capacity)
{
	bool known_prefix;

	if (!data || !capacity || size != LORGAR_STATE_REPORT_LEN)
		return false;
	known_prefix = (data[0] == 0x01 && data[1] < LORGAR_DPI_STAGE_COUNT) ||
		(data[0] == 0x02 && data[1] < LORGAR_DPI_STAGE_COUNT &&
		 (data[3] != 0x80 ||
		  (data[2] < LORGAR_DPI_STAGE_COUNT && data[8] <= 0x01)));
	if (!known_prefix || data[7] > 100)
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
