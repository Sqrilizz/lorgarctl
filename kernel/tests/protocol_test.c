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
	report[1] = 0x08;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[1] = 0x02;
	assert(lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[1] = 0x01;
	report[0] = 0x03;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[0] = 0x02;
	report[1] = 0x00;
	report[2] = 0x03;
	report[3] = 0x80;
	report[4] = 0x01;
	report[5] = 0x00;
	report[6] = 0x00;
	report[7] = 4;
	report[8] = 0x01;
	assert(lorgar_parse_state_report(report, sizeof(report), &capacity));
	assert(capacity == 4);
	report[2] = 1;
	assert(lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[2] = 8;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[2] = 3;
	report[7] = 101;
	assert(!lorgar_parse_state_report(report, sizeof(report), &capacity));
	report[7] = 56;
	report[8] = 0x00;
	assert(lorgar_parse_state_report(report, sizeof(report), &capacity));
	assert(capacity == 56);
	report[8] = 0x02;
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

static void battery_indicator_tests(void)
{
	uint8_t report[LORGAR_FEATURE_REPORT_LEN];

	lorgar_build_stage_color_report(report, 3, 82);
	assert(report[0] == LORGAR_CMD_STAGE_COLOR);
	assert(report[1] == 0);
	assert(report[2] == 3);
	assert(report[3] == 0x32);
	assert(report[4] == 0xcd);
	assert(report[5] == 0);
	assert(report[6] == 0);
	assert(report[7] == 0);

	lorgar_build_stage_color_report(report, 0, 20);
	assert(report[3] == 0xff && report[4] == 0);
	lorgar_build_stage_color_report(report, 0, 21);
	assert(report[3] == 0xff && report[4] == 0x8c);
	lorgar_build_stage_color_report(report, 0, 51);
	assert(report[3] == 0xff && report[4] == 0xd0);
	lorgar_build_stage_color_report(report, 0, 81);
	assert(report[3] == 0x32 && report[4] == 0xcd);
}

static void active_stage_tests(void)
{
	uint8_t report[LORGAR_FEATURE_REPORT_LEN];

	lorgar_build_active_stage_report(report, 5);
	assert(report[0] == LORGAR_CMD_ACTIVE_STAGE);
	assert(report[1] == 0);
	assert(report[2] == 0);
	assert(report[3] == 5);
	assert(report[4] == 0);
	assert(report[5] == 0);
	assert(report[6] == 0);
	assert(report[7] == 0);
}

int main(void)
{
	state_report_tests();
	dpi_event_tests();
	battery_indicator_tests();
	active_stage_tests();
	return 0;
}
