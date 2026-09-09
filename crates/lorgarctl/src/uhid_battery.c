#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <linux/uhid.h>

static unsigned char rdesc[] = {
    0x05, 0x85,        // USAGE_PAGE (Battery System)
    0x09, 0x01,        // USAGE (Smart Battery BatterySystem)
    0xa1, 0x01,        // COLLECTION (Application)
    0x85, 0x01,        //   REPORT_ID (1)
    0x05, 0x85,        //   USAGE_PAGE (Battery System)
    0x09, 0x66,        //   USAGE (Capacity Mode / Battery Level)
    0x15, 0x00,        //   LOGICAL_MINIMUM (0)
    0x26, 0x64, 0x00,  //   LOGICAL_MAXIMUM (100)
    0x75, 0x08,        //   REPORT_SIZE (8)
    0x95, 0x01,        //   REPORT_COUNT (1)
    0x81, 0x02,        //   INPUT (Data,Var,Abs)
    0xc0               // END_COLLECTION
};

int lorgar_uhid_open(void) {
    int fd = open("/dev/uhid", O_RDWR | O_CLOEXEC);
    if (fd < 0) return -1;

    struct uhid_event ev;
    memset(&ev, 0, sizeof(ev));
    ev.type = UHID_CREATE2;
    strncpy((char*)ev.u.create2.name, "LORGAR MSA10W Mouse", sizeof(ev.u.create2.name) - 1);
    ev.u.create2.rd_size = sizeof(rdesc);
    memcpy(ev.u.create2.rd_data, rdesc, sizeof(rdesc));
    ev.u.create2.bus = BUS_USB;
    ev.u.create2.vendor = 0x093a;
    ev.u.create2.product = 0xc601;

    if (write(fd, &ev, sizeof(ev)) != sizeof(ev)) {
        close(fd);
        return -1;
    }
    return fd;
}

int lorgar_uhid_update_battery(int fd, unsigned char percentage) {
    if (fd < 0) return -1;
    struct uhid_event ev;
    memset(&ev, 0, sizeof(ev));
    ev.type = UHID_INPUT2;
    ev.u.input2.size = 2;
    ev.u.input2.data[0] = 0x01; // report id
    ev.u.input2.data[1] = percentage;

    if (write(fd, &ev, sizeof(ev)) != sizeof(ev)) return -1;
    return 0;
}

void lorgar_uhid_close(int fd) {
    if (fd >= 0) close(fd);
}
