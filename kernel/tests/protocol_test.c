#include <assert.h>
#include <stdint.h>

#include "../hid-lorgar-protocol.h"

static void state_report_tests(void)
{
	uint8_t report[LORGAR_STATE_REPORT_LEN] = { 0x01, 0x01 };
	uint8_t capacity = 0;

	report[7] = 81;
	assert(lorgar_parse_state_report(report, sizeof(report), &capacity));
	assert(capacity == 81);
	report[0] = 0x02;
	report[7] = 82;
	assert(lorgar_parse_state_report(report, sizeof(report), &capacity));
	assert(capacity == 82);
	assert(!lorgar_parse_state_report(report, sizeof(report) - 1, &capacity));
	report[7] = 101;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[7] = 82;
	report[1] = 0x02;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[1] = 0x01;
	report[0] = 0x03;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
}

static void dpi_event_tests(void)
{
	uint8_t report[] = { 0x05, 0x0b, 0x03, 0x00, 0x00 };
	uint8_t stage = 0;

	assert(lorgar_parse_dpi_event(0x05, report, sizeof(report), &stage));
	assert(stage == 3);
	assert(!lorgar_parse_dpi_event(0x04, report, sizeof(report), &stage));
	report[1] = 0x0f;
	assert(!lorgar_parse_dpi_event(0x05, report, sizeof(report), &stage));
	report[1] = 0x0b;
	report[2] = 8;
	assert(!lorgar_parse_dpi_event(0x05, report, sizeof(report), &stage));
}

int main(void)
{
	state_report_tests();
	dpi_event_tests();
	return 0;
}
