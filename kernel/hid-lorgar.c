// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * HID driver for LORGAR MSA10W Wireless/Wired Gaming Mouse
 * VID: 0x093a (Pixart Imaging, Inc.)
 * PID: 0xc600 (Wired / Charging mode)
 * PID: 0xc601 (2.4G Wireless Dongle)
 *
 * Copyright (c) 2026 Sqrilizz
 */

#include <linux/module.h>
#include <linux/kernel.h>
#include <linux/init.h>
#include <linux/hid.h>
#include <linux/usb.h>
#include <linux/power_supply.h>
#include <linux/workqueue.h>
#include <linux/slab.h>
#include <linux/mutex.h>
#include <linux/kref.h>
#include <linux/list.h>

#define DRIVER_NAME "hid-lorgar"

#define USB_VENDOR_ID_LORGAR                   0x093a
#define USB_DEVICE_ID_LORGAR_MSA10W_WIRED      0xc600
#define USB_DEVICE_ID_LORGAR_MSA10W_WIRELESS   0xc601

#define LORGAR_CONFIG_REPORT_LEN 9
#define LORGAR_STATE_REPORT_LEN  64

/* Vendor Opcodes for Interface 2 (Feature Reports) */
#define LORGAR_CMD_POLL_RATE     0x20
#define LORGAR_CMD_ACTIVE_STAGE  0x21
#define LORGAR_CMD_LIGHT_MODE    0x22
#define LORGAR_CMD_LIGHT_COLOR   0x23
#define LORGAR_CMD_SET_DPI       0x26
#define LORGAR_CMD_QUERY_STATE   0x82

/* Vendor Input Events (Report ID 0x05) */
#define LORGAR_EVENT_REPORT_ID   0x05
#define LORGAR_EVENT_DPI_TOGGLE  0x0b

/* Refresh interval for battery polling in seconds */
#define LORGAR_BATTERY_POLL_INTERVAL_SEC 30

struct lorgar_drvdata;

/* Shared hardware connection state across all USB interfaces */
struct lorgar_device_ctx {
	struct list_head node;
	struct usb_device *udev;
	struct kref ref;
	struct mutex lock;
	unsigned long last_packet_time;
	bool is_online;
	struct lorgar_drvdata *config_drv;
};

static LIST_HEAD(lorgar_device_list);
static DEFINE_MUTEX(lorgar_list_lock);

struct lorgar_drvdata {
	struct hid_device *hdev;
	struct lorgar_device_ctx *ctx;
	struct power_supply *battery;
	struct power_supply_desc battery_desc;
	char battery_name[64];
	struct delayed_work battery_work;
	struct mutex lock;

	bool is_config_interface;
	bool is_wired;
	u8 battery_capacity;
	u8 active_dpi_stage;
	u16 stage_dpis[8];
	u8 polling_rate_hz_idx;
	u8 led_mode;
	u8 led_r;
	u8 led_g;
	u8 led_b;
	bool power_save;
};

static void lorgar_ctx_release(struct kref *kref)
{
	struct lorgar_device_ctx *ctx = container_of(kref, struct lorgar_device_ctx, ref);

	list_del(&ctx->node);
	kfree(ctx);
}

static struct lorgar_device_ctx *lorgar_get_or_create_ctx(struct usb_device *udev)
{
	struct lorgar_device_ctx *ctx;

	mutex_lock(&lorgar_list_lock);
	list_for_each_entry(ctx, &lorgar_device_list, node) {
		if (ctx->udev == udev) {
			kref_get(&ctx->ref);
			mutex_unlock(&lorgar_list_lock);
			return ctx;
		}
	}

	ctx = kzalloc(sizeof(*ctx), GFP_KERNEL);
	if (!ctx) {
		mutex_unlock(&lorgar_list_lock);
		return NULL;
	}

	ctx->udev = udev;
	kref_init(&ctx->ref);
	mutex_init(&ctx->lock);
	ctx->last_packet_time = jiffies;
	ctx->is_online = true;

	list_add(&ctx->node, &lorgar_device_list);
	mutex_unlock(&lorgar_list_lock);
	return ctx;
}

static void lorgar_put_ctx(struct lorgar_device_ctx *ctx)
{
	if (!ctx)
		return;

	mutex_lock(&lorgar_list_lock);
	kref_put(&ctx->ref, lorgar_ctx_release);
	mutex_unlock(&lorgar_list_lock);
}

static const enum power_supply_property lorgar_battery_props[] = {
	POWER_SUPPLY_PROP_STATUS,
	POWER_SUPPLY_PROP_PRESENT,
	POWER_SUPPLY_PROP_CAPACITY,
	POWER_SUPPLY_PROP_SCOPE,
	POWER_SUPPLY_PROP_MODEL_NAME,
	POWER_SUPPLY_PROP_MANUFACTURER,
};

static int lorgar_send_feature(struct lorgar_drvdata *drv, const u8 *payload, size_t len)
{
	u8 *buf;
	int ret;

	if (len > 8)
		return -EINVAL;

	buf = kmalloc(LORGAR_CONFIG_REPORT_LEN, GFP_KERNEL);
	if (!buf)
		return -ENOMEM;

	memset(buf, 0, LORGAR_CONFIG_REPORT_LEN);
	buf[0] = 0x00; /* Report ID 0 */
	memcpy(&buf[1], payload, len);

	mutex_lock(&drv->lock);
	ret = hid_hw_raw_request(drv->hdev, 0x00, buf, LORGAR_CONFIG_REPORT_LEN,
				 HID_FEATURE_REPORT, HID_REQ_SET_REPORT);
	mutex_unlock(&drv->lock);

	kfree(buf);
	return (ret < 0) ? ret : 0;
}

static void lorgar_query_state(struct lorgar_drvdata *drv)
{
	u8 cmd[8] = { LORGAR_CMD_QUERY_STATE, 0, 0, 0, 0, 0, 0, 0 };
	int ret;

	ret = lorgar_send_feature(drv, cmd, sizeof(cmd));
	if (ret < 0 && drv->ctx) {
		/* Hardware didn't respond -> mouse is asleep */
		bool changed = false;
		mutex_lock(&drv->ctx->lock);
		if (drv->ctx->is_online) {
			drv->ctx->is_online = false;
			changed = true;
		}
		mutex_unlock(&drv->ctx->lock);

		if (changed) {
			sysfs_notify(&drv->hdev->dev.kobj, NULL, "hardware_status");
			kobject_uevent(&drv->hdev->dev.kobj, KOBJ_CHANGE);
		}
	}
}

static void lorgar_battery_work(struct work_struct *work)
{
	struct lorgar_drvdata *drv = container_of(to_delayed_work(work),
						  struct lorgar_drvdata,
						  battery_work);

	lorgar_query_state(drv);

	schedule_delayed_work(&drv->battery_work,
			      msecs_to_jiffies(LORGAR_BATTERY_POLL_INTERVAL_SEC * 1000));
}

static int lorgar_battery_get_property(struct power_supply *psy,
				       enum power_supply_property psp,
				       union power_supply_propval *val)
{
	struct lorgar_drvdata *drv = power_supply_get_drvdata(psy);

	switch (psp) {
	case POWER_SUPPLY_PROP_STATUS:
		if (drv->is_wired) {
			if (drv->battery_capacity >= 100)
				val->intval = POWER_SUPPLY_STATUS_FULL;
			else
				val->intval = POWER_SUPPLY_STATUS_CHARGING;
		} else {
			if (drv->battery_capacity >= 100)
				val->intval = POWER_SUPPLY_STATUS_FULL;
			else
				val->intval = POWER_SUPPLY_STATUS_DISCHARGING;
		}
		break;
	case POWER_SUPPLY_PROP_PRESENT:
		val->intval = 1;
		break;
	case POWER_SUPPLY_PROP_CAPACITY:
		val->intval = drv->battery_capacity;
		break;
	case POWER_SUPPLY_PROP_SCOPE:
		val->intval = POWER_SUPPLY_SCOPE_DEVICE;
		break;
	case POWER_SUPPLY_PROP_MODEL_NAME:
		val->strval = "LORGAR MSA10W";
		break;
	case POWER_SUPPLY_PROP_MANUFACTURER:
		val->strval = "LORGAR";
		break;
	default:
		return -EINVAL;
	}

	return 0;
}

/* Sysfs: Hardware Link Status (online = responding, asleep = radio off) */
static ssize_t hardware_status_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	bool online;

	if (!drv || !drv->ctx)
		return -ENODEV;

	mutex_lock(&drv->ctx->lock);
	online = drv->ctx->is_online;
	mutex_unlock(&drv->ctx->lock);

	return sysfs_emit(buf, "%s\n", online ? "online" : "asleep");
}
static DEVICE_ATTR_RO(hardware_status);

/* Sysfs: Active DPI Value (read/write direct resolution in DPI, e.g. 800, 1600) */
static ssize_t dpi_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	u16 current_dpi;

	if (!drv)
		return -ENODEV;

	current_dpi = drv->stage_dpis[drv->active_dpi_stage];
	return sysfs_emit(buf, "%u\n", current_dpi);
}

static ssize_t dpi_store(struct device *dev, struct device_attribute *attr,
			 const char *buf, size_t count)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	u32 target_dpi;
	u8 cmd[8] = { 0 };
	int ret;

	if (!drv)
		return -ENODEV;

	ret = kstrtou32(buf, 0, &target_dpi);
	if (ret < 0 || target_dpi < 100 || target_dpi > 26000)
		return -EINVAL;

	/* Command 0x26: Set DPI value for stage */
	cmd[0] = LORGAR_CMD_SET_DPI;
	cmd[1] = 0x00;
	cmd[2] = drv->active_dpi_stage;
	cmd[3] = target_dpi & 0xff;
	cmd[4] = (target_dpi >> 8) & 0xff;

	ret = lorgar_send_feature(drv, cmd, sizeof(cmd));
	if (ret < 0)
		return ret;

	drv->stage_dpis[drv->active_dpi_stage] = (u16)target_dpi;
	return count;
}
static DEVICE_ATTR_RW(dpi);

/* Sysfs: Active DPI Stage Index (0..7) */
static ssize_t dpi_stage_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);

	if (!drv)
		return -ENODEV;

	return sysfs_emit(buf, "%u\n", drv->active_dpi_stage);
}

static ssize_t dpi_stage_store(struct device *dev, struct device_attribute *attr,
			       const char *buf, size_t count)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	u8 stage;
	u8 cmd[8] = { 0 };
	int ret;

	if (!drv)
		return -ENODEV;

	ret = kstrtou8(buf, 0, &stage);
	if (ret < 0 || stage > 7)
		return -EINVAL;

	cmd[0] = LORGAR_CMD_ACTIVE_STAGE;
	cmd[3] = stage;

	ret = lorgar_send_feature(drv, cmd, sizeof(cmd));
	if (ret < 0)
		return ret;

	drv->active_dpi_stage = stage;
	sysfs_notify(&hdev->dev.kobj, NULL, "dpi");
	return count;
}
static DEVICE_ATTR_RW(dpi_stage);

/* Sysfs: Polling Rate (125, 250, 500, 1000 Hz) */
static ssize_t polling_rate_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	unsigned int hz = 1000;

	if (!drv)
		return -ENODEV;

	switch (drv->polling_rate_hz_idx) {
	case 0x01: hz = 1000; break;
	case 0x02: hz = 500;  break;
	case 0x04: hz = 250;  break;
	case 0x08: hz = 125;  break;
	default:   hz = 1000; break;
	}

	return sysfs_emit(buf, "%u\n", hz);
}

static ssize_t polling_rate_store(struct device *dev, struct device_attribute *attr,
				  const char *buf, size_t count)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	unsigned int hz;
	u8 interval;
	u8 cmd[8] = { 0 };
	int ret;

	if (!drv)
		return -ENODEV;

	ret = kstrtouint(buf, 0, &hz);
	if (ret < 0)
		return -EINVAL;

	switch (hz) {
	case 1000: interval = 0x01; break;
	case 500:  interval = 0x02; break;
	case 250:  interval = 0x04; break;
	case 125:  interval = 0x08; break;
	default:   return -EINVAL;
	}

	cmd[0] = LORGAR_CMD_POLL_RATE;
	cmd[3] = interval;

	ret = lorgar_send_feature(drv, cmd, sizeof(cmd));
	if (ret < 0)
		return ret;

	drv->polling_rate_hz_idx = interval;
	return count;
}
static DEVICE_ATTR_RW(polling_rate);

/* Sysfs: LED Lighting Mode (1: Static, 2: Breathing, 3: Cycle, 4: Tapping, 5: Wave) */
static ssize_t led_mode_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);

	if (!drv)
		return -ENODEV;

	return sysfs_emit(buf, "%u\n", drv->led_mode);
}

static ssize_t led_mode_store(struct device *dev, struct device_attribute *attr,
			      const char *buf, size_t count)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	u8 mode;
	u8 param = 0x05;
	u8 cmd[8] = { 0 };
	int ret;

	if (!drv)
		return -ENODEV;

	ret = kstrtou8(buf, 0, &mode);
	if (ret < 0 || mode < 1 || mode > 5)
		return -EINVAL;

	if (mode == 5)
		param = 0x01; /* Wave mode uses param 0x01 */

	cmd[0] = LORGAR_CMD_LIGHT_MODE;
	cmd[3] = mode;
	cmd[4] = 0x80;
	cmd[5] = param;

	ret = lorgar_send_feature(drv, cmd, sizeof(cmd));
	if (ret < 0)
		return ret;

	drv->led_mode = mode;
	return count;
}
static DEVICE_ATTR_RW(led_mode);

/* Sysfs: LED Color (HEX RRGGBB, 000000 = lights completely OFF) */
static ssize_t led_color_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);

	if (!drv)
		return -ENODEV;

	return sysfs_emit(buf, "%02x%02x%02x\n", drv->led_r, drv->led_g, drv->led_b);
}

static ssize_t led_color_store(struct device *dev, struct device_attribute *attr,
			       const char *buf, size_t count)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	u32 color;
	u8 r, g, b;
	u8 cmd[8] = { 0 };
	int ret;

	if (!drv)
		return -ENODEV;

	ret = kstrtou32(buf, 16, &color);
	if (ret < 0 || color > 0xffffff)
		return -EINVAL;

	r = (color >> 16) & 0xff;
	g = (color >> 8) & 0xff;
	b = color & 0xff;

	cmd[0] = LORGAR_CMD_LIGHT_COLOR;
	cmd[3] = r;
	cmd[4] = g;
	cmd[5] = b;

	ret = lorgar_send_feature(drv, cmd, sizeof(cmd));
	if (ret < 0)
		return ret;

	drv->led_r = r;
	drv->led_g = g;
	drv->led_b = b;
	return count;
}
static DEVICE_ATTR_RW(led_color);

/* Sysfs: Eco / Power-Save Mode (1 = RGB off & 500 Hz, 0 = normal 1000 Hz) */
static ssize_t power_save_show(struct device *dev, struct device_attribute *attr, char *buf)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);

	if (!drv)
		return -ENODEV;

	return sysfs_emit(buf, "%u\n", drv->power_save ? 1 : 0);
}

static ssize_t power_save_store(struct device *dev, struct device_attribute *attr,
				const char *buf, size_t count)
{
	struct hid_device *hdev = to_hid_device(dev);
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	bool enable;
	int ret;

	if (!drv)
		return -ENODEV;

	ret = kstrtobool(buf, &enable);
	if (ret < 0)
		return -EINVAL;

	drv->power_save = enable;

	if (enable) {
		/* Turn off LED and set 500 Hz for 80+ hours of battery life */
		u8 led_cmd[8] = { LORGAR_CMD_LIGHT_COLOR, 0, 0, 0, 0, 0, 0, 0 };
		u8 poll_cmd[8] = { LORGAR_CMD_POLL_RATE, 0, 0, 0x02, 0, 0, 0, 0 };

		lorgar_send_feature(drv, led_cmd, sizeof(led_cmd));
		lorgar_send_feature(drv, poll_cmd, sizeof(poll_cmd));
		drv->led_r = drv->led_g = drv->led_b = 0;
		drv->polling_rate_hz_idx = 0x02;
	} else {
		/* Restore 1000 Hz */
		u8 poll_cmd[8] = { LORGAR_CMD_POLL_RATE, 0, 0, 0x01, 0, 0, 0, 0 };
		lorgar_send_feature(drv, poll_cmd, sizeof(poll_cmd));
		drv->polling_rate_hz_idx = 0x01;
	}

	return count;
}
static DEVICE_ATTR_RW(power_save);

static struct attribute *lorgar_attrs[] = {
	&dev_attr_hardware_status.attr,
	&dev_attr_dpi.attr,
	&dev_attr_dpi_stage.attr,
	&dev_attr_polling_rate.attr,
	&dev_attr_led_mode.attr,
	&dev_attr_led_color.attr,
	&dev_attr_power_save.attr,
	NULL,
};

static const struct attribute_group lorgar_attr_group = {
	.attrs = lorgar_attrs,
};

static int lorgar_raw_event(struct hid_device *hdev, struct hid_report *report,
			    u8 *data, int size)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	struct lorgar_device_ctx *ctx;
	bool was_asleep = false;

	if (!drv)
		return 0;

	ctx = drv->ctx;
	if (ctx) {
		mutex_lock(&ctx->lock);
		was_asleep = !ctx->is_online;
		ctx->last_packet_time = jiffies;
		ctx->is_online = true;
		mutex_unlock(&ctx->lock);

		if (was_asleep && ctx->config_drv) {
			sysfs_notify(&ctx->config_drv->hdev->dev.kobj, NULL, "hardware_status");
			kobject_uevent(&ctx->config_drv->hdev->dev.kobj, KOBJ_CHANGE);
		}
	}

	/* Interface 2 vendor state response: 64 bytes unnumbered [01 01 05 ...] */
	if (drv->is_config_interface && size >= 64 && data[0] == 0x01 && data[1] == 0x01) {
		u8 capacity = data[7];

		if (capacity <= 100) {
			if (drv->battery_capacity != capacity) {
				drv->battery_capacity = capacity;
				if (drv->battery)
					power_supply_changed(drv->battery);
			}
		}
		return 0;
	}

	/* Vendor input event report 0x05 */
	if (report && report->id == LORGAR_EVENT_REPORT_ID && size >= 3) {
		u8 event_type = data[0];
		u8 stage = data[1];

		if (event_type == LORGAR_EVENT_DPI_TOGGLE) {
			drv->active_dpi_stage = stage;
			sysfs_notify(&hdev->dev.kobj, NULL, "dpi_stage");
			sysfs_notify(&hdev->dev.kobj, NULL, "dpi");
		}
	}

	return 0;
}

static bool lorgar_is_config_interface(struct hid_device *hdev)
{
	struct usb_interface *intf;

	if (!hid_is_usb(hdev))
		return false;

	intf = to_usb_interface(hdev->dev.parent);
	if (!intf || !intf->cur_altsetting)
		return false;

	return intf->cur_altsetting->desc.bInterfaceNumber == 2;
}

static int lorgar_probe(struct hid_device *hdev, const struct hid_device_id *id)
{
	struct lorgar_drvdata *drv;
	struct lorgar_device_ctx *ctx = NULL;
	struct power_supply_config psy_cfg = { 0 };
	struct usb_interface *intf;
	struct usb_device *udev = NULL;
	int ret;

	ret = hid_parse(hdev);
	if (ret) {
		hid_err(hdev, "parse failed\n");
		return ret;
	}

	ret = hid_hw_start(hdev, HID_CONNECT_DEFAULT);
	if (ret) {
		hid_err(hdev, "hw start failed\n");
		return ret;
	}

	if (hid_is_usb(hdev)) {
		intf = to_usb_interface(hdev->dev.parent);
		if (intf)
			udev = interface_to_usbdev(intf);
	}

	if (udev)
		ctx = lorgar_get_or_create_ctx(udev);

	drv = devm_kzalloc(&hdev->dev, sizeof(*drv), GFP_KERNEL);
	if (!drv) {
		ret = -ENOMEM;
		goto err_ctx;
	}

	drv->hdev = hdev;
	drv->ctx = ctx;
	drv->is_config_interface = lorgar_is_config_interface(hdev);
	drv->is_wired = (id->product == USB_DEVICE_ID_LORGAR_MSA10W_WIRED);
	drv->battery_capacity = 100;
	drv->polling_rate_hz_idx = 0x01; /* 1000 Hz */
	drv->led_mode = 1;

	/* Default MSA10W DPI stages */
	drv->stage_dpis[0] = 800;
	drv->stage_dpis[1] = 1200;
	drv->stage_dpis[2] = 2400;
	drv->stage_dpis[3] = 3200;
	drv->stage_dpis[4] = 6400;
	drv->stage_dpis[5] = 12000;
	drv->stage_dpis[6] = 18000;
	drv->stage_dpis[7] = 26000;

	mutex_init(&drv->lock);

	hid_set_drvdata(hdev, drv);

	if (drv->is_config_interface) {
		if (ctx)
			ctx->config_drv = drv;

		/* Register sysfs group */
		ret = sysfs_create_group(&hdev->dev.kobj, &lorgar_attr_group);
		if (ret) {
			hid_err(hdev, "failed to create sysfs group\n");
			goto err_ctx;
		}

		/* Register power supply */
		snprintf(drv->battery_name, sizeof(drv->battery_name),
			 "lorgar_mouse_battery_%s", dev_name(&hdev->dev));

		drv->battery_desc.name = drv->battery_name;
		drv->battery_desc.type = POWER_SUPPLY_TYPE_BATTERY;
		drv->battery_desc.properties = lorgar_battery_props;
		drv->battery_desc.num_properties = ARRAY_SIZE(lorgar_battery_props);
		drv->battery_desc.get_property = lorgar_battery_get_property;

		psy_cfg.drv_data = drv;
		drv->battery = devm_power_supply_register(&hdev->dev,
							  &drv->battery_desc,
							  &psy_cfg);
		if (IS_ERR(drv->battery)) {
			ret = PTR_ERR(drv->battery);
			hid_err(hdev, "failed to register power supply: %d\n", ret);
			drv->battery = NULL;
			goto err_sysfs;
		}

		INIT_DELAYED_WORK(&drv->battery_work, lorgar_battery_work);

		/* Query initial state and start periodic timer */
		lorgar_query_state(drv);
		schedule_delayed_work(&drv->battery_work, msecs_to_jiffies(1000));

		hid_info(hdev, "LORGAR MSA10W hardware driver registered successfully\n");
	}

	return 0;

err_sysfs:
	sysfs_remove_group(&hdev->dev.kobj, &lorgar_attr_group);
err_ctx:
	if (ctx)
		lorgar_put_ctx(ctx);
	hid_hw_stop(hdev);
	return ret;
}

static void lorgar_remove(struct hid_device *hdev)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);

	if (drv) {
		if (drv->is_config_interface) {
			cancel_delayed_work_sync(&drv->battery_work);
			sysfs_remove_group(&hdev->dev.kobj, &lorgar_attr_group);
			if (drv->ctx)
				drv->ctx->config_drv = NULL;
		}

		if (drv->ctx) {
			lorgar_put_ctx(drv->ctx);
			drv->ctx = NULL;
		}
	}

	hid_hw_stop(hdev);
}

static const struct hid_device_id lorgar_devices[] = {
	{ HID_USB_DEVICE(USB_VENDOR_ID_LORGAR, USB_DEVICE_ID_LORGAR_MSA10W_WIRED) },
	{ HID_USB_DEVICE(USB_VENDOR_ID_LORGAR, USB_DEVICE_ID_LORGAR_MSA10W_WIRELESS) },
	{ }
};
MODULE_DEVICE_TABLE(hid, lorgar_devices);

static struct hid_driver lorgar_driver = {
	.name = DRIVER_NAME,
	.id_table = lorgar_devices,
	.probe = lorgar_probe,
	.remove = lorgar_remove,
	.raw_event = lorgar_raw_event,
};
module_hid_driver(lorgar_driver);

MODULE_AUTHOR("Sqrilizz");
MODULE_DESCRIPTION("HID Driver for LORGAR MSA10W Wireless/Wired Gaming Mouse");
MODULE_LICENSE("GPL");
MODULE_VERSION("1.3.0");
