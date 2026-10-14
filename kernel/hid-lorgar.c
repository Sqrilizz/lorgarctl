// SPDX-License-Identifier: GPL-2.0-or-later
/*
 * HID driver for LORGAR MSA10W Wireless/Wired Gaming Mouse
 * VID: 0x093a (Pixart Imaging, Inc.)
 * PID: 0xc600 (Wired) / 0xc601 (2.4 GHz receiver)
 *
 * Copyright (c) 2026 Sqrilizz
 */

#include <linux/delay.h>
#include <linux/hid.h>
#include <linux/init.h>
#include <linux/jiffies.h>
#include <linux/kernel.h>
#include <linux/kref.h>
#include <linux/list.h>
#include <linux/module.h>
#include <linux/mutex.h>
#include <linux/power_supply.h>
#include <linux/slab.h>
#include <linux/spinlock.h>
#include <linux/usb.h>
#include <linux/workqueue.h>

#include "hid-lorgar-protocol.h"

#define DRIVER_NAME "hid-lorgar"

#define USB_VENDOR_ID_LORGAR 0x093a
#define USB_DEVICE_ID_LORGAR_MSA10W_WIRED 0xc600
#define USB_DEVICE_ID_LORGAR_MSA10W_WIRELESS 0xc601

#define LORGAR_MIN_DPI 100
#define LORGAR_MAX_DPI 12000

#define LORGAR_CMD_POLL_RATE 0x20
#define LORGAR_CMD_ACTIVE_STAGE 0x21
#define LORGAR_CMD_LIGHT_MODE 0x22
#define LORGAR_CMD_LIGHT_COLOR 0x23
#define LORGAR_CMD_SET_DPI 0x26
#define LORGAR_CMD_QUERY_STATE 0x82

#define LORGAR_QUERY_INTERVAL_MS 60000
#define LORGAR_QUERY_TIMEOUT_MS 2000
#define LORGAR_STARTUP_QUERY_RETRY_MS 500
#define LORGAR_STARTUP_QUERY_ATTEMPTS 3
#define LORGAR_QUERY_TIMEOUTS_BEFORE_OFFLINE 2
#define LORGAR_COMMAND_COOLDOWN_MS 20

#define LORGAR_NOTIFY_LINK BIT(0)
#define LORGAR_NOTIFY_BATTERY BIT(1)
#define LORGAR_NOTIFY_DPI BIT(2)
#define LORGAR_NOTIFY_INPUT BIT(3)

enum lorgar_link_state {
	LORGAR_LINK_UNKNOWN,
	LORGAR_LINK_ONLINE,
	LORGAR_LINK_UNRESPONSIVE,
};

struct lorgar_shared_state {
	spinlock_t lock;
	enum lorgar_link_state link_state;
	bool query_pending;
	unsigned long notifications;
	unsigned long query_deadline;
	unsigned long next_query;
	bool battery_valid;
	u8 battery_capacity;
	bool active_stage_valid;
	u8 active_dpi_stage;
	bool stage_dpi_valid[LORGAR_DPI_STAGE_COUNT];
	u16 stage_dpis[LORGAR_DPI_STAGE_COUNT];
	bool polling_rate_valid;
	u8 polling_rate_interval;
	bool led_mode_valid;
	u8 led_mode;
	bool led_color_valid;
	u8 led_r;
	u8 led_g;
	u8 led_b;
	bool battery_indicator_enabled;
	u8 query_timeouts;
	bool input_seen;
};

struct lorgar_device_ctx {
	struct list_head node;
	struct usb_device *udev;
	struct kref ref;
	struct lorgar_shared_state state;
	struct hid_device *config_hdev;
};

struct lorgar_drvdata {
	struct hid_device *hdev;
	struct lorgar_device_ctx *ctx;
	struct power_supply *battery;
	struct power_supply_desc battery_desc;
	char battery_name[64];
	struct delayed_work battery_work;
	struct delayed_work indicator_work;
	struct work_struct notification_work;
	struct mutex io_lock;
	unsigned long last_command;
	bool indicator_band_valid;
	u8 indicator_band;
	u8 startup_query_attempts;
	int interface_number;
	bool is_config_interface;
};

static LIST_HEAD(lorgar_device_list);
static DEFINE_MUTEX(lorgar_list_lock);

static void lorgar_ctx_release(struct kref *ref)
{
	struct lorgar_device_ctx *ctx =
		container_of(ref, struct lorgar_device_ctx, ref);

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
	spin_lock_init(&ctx->state.lock);
	ctx->state.link_state = LORGAR_LINK_UNKNOWN;
	ctx->state.next_query = jiffies;
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

static void lorgar_schedule_notification(struct lorgar_device_ctx *ctx,
					 unsigned long notifications)
{
	struct lorgar_drvdata *drv = NULL;
	unsigned long flags;

	spin_lock_irqsave(&ctx->state.lock, flags);
	ctx->state.notifications |= notifications;
	if (ctx->config_hdev)
		drv = hid_get_drvdata(ctx->config_hdev);
	if (drv)
		schedule_work(&drv->notification_work);
	spin_unlock_irqrestore(&ctx->state.lock, flags);
}

static void lorgar_kick_indicator(struct lorgar_device_ctx *ctx)
{
	struct lorgar_drvdata *drv = NULL;
	unsigned long flags;

	spin_lock_irqsave(&ctx->state.lock, flags);
	if (ctx->config_hdev)
		drv = hid_get_drvdata(ctx->config_hdev);
	if (drv)
		mod_delayed_work(system_dfl_wq, &drv->indicator_work, 0);
	spin_unlock_irqrestore(&ctx->state.lock, flags);
}

static const enum power_supply_property lorgar_battery_props[] = {
	POWER_SUPPLY_PROP_STATUS,
	POWER_SUPPLY_PROP_PRESENT,
	POWER_SUPPLY_PROP_CAPACITY,
	POWER_SUPPLY_PROP_SCOPE,
	POWER_SUPPLY_PROP_MODEL_NAME,
	POWER_SUPPLY_PROP_MANUFACTURER,
};

static int lorgar_register_battery(struct lorgar_drvdata *drv)
{
	struct power_supply_config supply_config = { .drv_data = drv };
	struct power_supply_desc *desc = &drv->battery_desc;
	struct device *dev = &drv->hdev->dev;
	struct power_supply *battery;

	if (drv->battery)
		return 0;
	battery = devm_power_supply_register(dev, desc, &supply_config);
	if (IS_ERR(battery))
		return PTR_ERR(battery);
	drv->battery = battery;
	return 0;
}

static void lorgar_notification_work(struct work_struct *work)
{
	struct lorgar_drvdata *drv =
		container_of(work, struct lorgar_drvdata, notification_work);
	struct lorgar_shared_state *shared = &drv->ctx->state;
	unsigned long flags;
	unsigned long notifications;
	bool battery_valid;
	bool battery_registered = false;
	int ret;

	spin_lock_irqsave(&shared->lock, flags);
	notifications = shared->notifications;
	shared->notifications = 0;
	battery_valid = shared->battery_valid;
	spin_unlock_irqrestore(&shared->lock, flags);
	if ((notifications & LORGAR_NOTIFY_BATTERY) && battery_valid &&
	    !drv->battery) {
		ret = lorgar_register_battery(drv);
		if (ret < 0)
			hid_warn(drv->hdev,
				 "battery registration failed: %d\n", ret);
		else
			battery_registered = true;
	}
	if (notifications & LORGAR_NOTIFY_LINK)
		sysfs_notify(&drv->hdev->dev.kobj, NULL, "hardware_status");
	if (notifications & LORGAR_NOTIFY_INPUT)
		sysfs_notify(&drv->hdev->dev.kobj, NULL, "input_seen");
	if (notifications & LORGAR_NOTIFY_DPI) {
		sysfs_notify(&drv->hdev->dev.kobj, NULL, "dpi_stage");
		sysfs_notify(&drv->hdev->dev.kobj, NULL, "dpi");
	}
	if ((notifications & LORGAR_NOTIFY_BATTERY) && drv->battery &&
	    !battery_registered)
		power_supply_changed(drv->battery);
	if (notifications)
		kobject_uevent(&drv->hdev->dev.kobj, KOBJ_CHANGE);
}

static int lorgar_send_feature(struct lorgar_drvdata *drv, const u8 *payload,
			       size_t len)
{
	u8 *report;
	unsigned long earliest;
	unsigned long remaining;
	int ret;

	if (!drv->is_config_interface)
		return -ENODEV;
	if (len != LORGAR_FEATURE_REPORT_LEN)
		return -EINVAL;
	report = kmemdup(payload, len, GFP_KERNEL);
	if (!report)
		return -ENOMEM;

	mutex_lock(&drv->io_lock);
	earliest = drv->last_command + msecs_to_jiffies(LORGAR_COMMAND_COOLDOWN_MS);
	if (time_before(jiffies, earliest)) {
		remaining = earliest - jiffies;
		msleep(jiffies_to_msecs(remaining) + 1);
	}
	ret = hid_hw_raw_request(drv->hdev, 0x00, report, len,
				 HID_FEATURE_REPORT, HID_REQ_SET_REPORT);
	drv->last_command = jiffies;
	mutex_unlock(&drv->io_lock);
	kfree(report);
	if (ret < 0)
		return ret;
	return ret == (int)len ? 0 : -EIO;
}

static void lorgar_indicator_work(struct work_struct *work)
{
	struct lorgar_drvdata *drv =
		container_of(to_delayed_work(work), struct lorgar_drvdata,
			     indicator_work);
	struct lorgar_shared_state *shared = &drv->ctx->state;
	u8 color[LORGAR_FEATURE_REPORT_LEN];
	unsigned long flags;
	u8 capacity;
	u8 band;
	bool enabled;
	bool battery_valid;
	bool online;
	bool palette_current;
	int ret;
	int i;

	spin_lock_irqsave(&shared->lock, flags);
	enabled = shared->battery_indicator_enabled;
	battery_valid = shared->battery_valid;
	capacity = shared->battery_capacity;
	online = shared->link_state == LORGAR_LINK_ONLINE;
	spin_unlock_irqrestore(&shared->lock, flags);
	if (!enabled || !online || !battery_valid)
		return;
	band = lorgar_battery_indicator_band(capacity);
	palette_current = drv->indicator_band_valid &&
		drv->indicator_band == band;
	if (palette_current)
		return;
	for (i = 0; i < LORGAR_DPI_STAGE_COUNT; i++) {
		lorgar_build_stage_color_report(color, i, capacity);
		ret = lorgar_send_feature(drv, color, sizeof(color));
		if (ret < 0) {
			hid_warn(drv->hdev,
				 "battery indicator color failed: %d\n", ret);
			return;
		}
	}
	drv->indicator_band = band;
	drv->indicator_band_valid = true;
}

static bool lorgar_note_timeout(struct lorgar_shared_state *shared)
{
	if (shared->query_timeouts < LORGAR_QUERY_TIMEOUTS_BEFORE_OFFLINE)
		shared->query_timeouts++;
	if (shared->query_timeouts < LORGAR_QUERY_TIMEOUTS_BEFORE_OFFLINE)
		return false;
	if (shared->link_state == LORGAR_LINK_UNRESPONSIVE)
		return false;
	shared->link_state = LORGAR_LINK_UNRESPONSIVE;
	return true;
}

static void lorgar_note_response(struct lorgar_shared_state *shared)
{
	shared->query_timeouts = 0;
}

static int lorgar_begin_query(struct lorgar_drvdata *drv)
{
	struct lorgar_shared_state *shared = &drv->ctx->state;
	u8 command[8] = { LORGAR_CMD_QUERY_STATE };
	unsigned long flags;
	bool link_changed = false;
	int ret;

	spin_lock_irqsave(&shared->lock, flags);
	if (shared->query_pending || time_before(jiffies, shared->next_query)) {
		spin_unlock_irqrestore(&shared->lock, flags);
		return 0;
	}
	shared->query_pending = true;
	shared->query_deadline = jiffies + msecs_to_jiffies(LORGAR_QUERY_TIMEOUT_MS);
	if (drv->startup_query_attempts < LORGAR_STARTUP_QUERY_ATTEMPTS)
		drv->startup_query_attempts++;
	spin_unlock_irqrestore(&shared->lock, flags);

	ret = lorgar_send_feature(drv, command, sizeof(command));
	if (ret < 0) {
		spin_lock_irqsave(&shared->lock, flags);
		shared->query_pending = false;
		shared->next_query = jiffies + msecs_to_jiffies(LORGAR_QUERY_INTERVAL_MS);
		if (lorgar_note_timeout(shared))
			link_changed = true;
		spin_unlock_irqrestore(&shared->lock, flags);
		hid_warn_ratelimited(drv->hdev, "state query write failed: %d\n",
				     ret);
		if (link_changed)
			lorgar_schedule_notification(drv->ctx,
					     LORGAR_NOTIFY_LINK |
					     LORGAR_NOTIFY_BATTERY);
	}
	return ret;
}

static void lorgar_battery_work(struct work_struct *work)
{
	struct lorgar_drvdata *drv =
		container_of(to_delayed_work(work), struct lorgar_drvdata,
			     battery_work);
	struct lorgar_shared_state *shared = &drv->ctx->state;
	unsigned long flags;
	unsigned long delay;
	unsigned int next_delay_ms;
	bool timed_out = false;
	bool link_changed = false;
	bool query_now = false;
	bool retry_now = false;

	spin_lock_irqsave(&shared->lock, flags);
	if (shared->query_pending) {
		if (time_before(jiffies, shared->query_deadline)) {
			delay = shared->query_deadline - jiffies;
		} else {
			shared->query_pending = false;
			retry_now = !shared->battery_valid &&
				drv->startup_query_attempts <
					LORGAR_STARTUP_QUERY_ATTEMPTS;
			next_delay_ms = retry_now ? LORGAR_STARTUP_QUERY_RETRY_MS :
				LORGAR_QUERY_INTERVAL_MS;
			shared->next_query = jiffies +
				msecs_to_jiffies(next_delay_ms);
			if (lorgar_note_timeout(shared))
				link_changed = true;
			delay = msecs_to_jiffies(next_delay_ms);
			timed_out = true;
		}
	} else if (time_before(jiffies, shared->next_query)) {
		delay = shared->next_query - jiffies;
	} else {
		delay = msecs_to_jiffies(LORGAR_QUERY_TIMEOUT_MS);
		query_now = true;
	}
	spin_unlock_irqrestore(&shared->lock, flags);

	if (timed_out) {
		hid_warn_ratelimited(drv->hdev,
				     "state query timed out after %u ms\n",
				     LORGAR_QUERY_TIMEOUT_MS);
		if (link_changed)
			lorgar_schedule_notification(drv->ctx,
						     LORGAR_NOTIFY_LINK |
						     LORGAR_NOTIFY_BATTERY);
	}
	if (query_now && lorgar_begin_query(drv) < 0)
		delay = msecs_to_jiffies(LORGAR_QUERY_INTERVAL_MS);
	mod_delayed_work(system_dfl_wq, &drv->battery_work,
			 max_t(unsigned long, 1, delay));
}

static int lorgar_battery_get_property(struct power_supply *psy,
				       enum power_supply_property property,
				       union power_supply_propval *value)
{
	struct lorgar_drvdata *drv = power_supply_get_drvdata(psy);
	struct lorgar_shared_state *shared = &drv->ctx->state;
	unsigned long flags;
	bool battery_valid;
	u8 capacity;

	spin_lock_irqsave(&shared->lock, flags);
	battery_valid = shared->battery_valid;
	capacity = shared->battery_capacity;
	spin_unlock_irqrestore(&shared->lock, flags);

	switch (property) {
	case POWER_SUPPLY_PROP_STATUS:
		value->intval = POWER_SUPPLY_STATUS_UNKNOWN;
		break;
	case POWER_SUPPLY_PROP_PRESENT:
		value->intval = battery_valid;
		break;
	case POWER_SUPPLY_PROP_CAPACITY:
		if (!battery_valid)
			return -ENODATA;
		value->intval = capacity;
		break;
	case POWER_SUPPLY_PROP_SCOPE:
		value->intval = POWER_SUPPLY_SCOPE_DEVICE;
		break;
	case POWER_SUPPLY_PROP_MODEL_NAME:
		value->strval = "LORGAR MSA10W";
		break;
	case POWER_SUPPLY_PROP_MANUFACTURER:
		value->strval = "LORGAR";
		break;
	default:
		return -EINVAL;
	}
	return 0;
}

static ssize_t hardware_status_show(struct device *dev,
				    struct device_attribute *attribute, char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	enum lorgar_link_state state;
	unsigned long flags;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	state = drv->ctx->state.link_state;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	if (state == LORGAR_LINK_ONLINE)
		return sysfs_emit(buf, "online\n");
	if (state == LORGAR_LINK_UNRESPONSIVE)
		return sysfs_emit(buf, "unresponsive\n");
	return sysfs_emit(buf, "unknown\n");
}
static DEVICE_ATTR_RO(hardware_status);

static ssize_t input_seen_show(struct device *dev,
			       struct device_attribute *attribute, char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	bool seen;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	seen = drv->ctx->state.input_seen;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return sysfs_emit(buf, "%u\n", seen);
}
static DEVICE_ATTR_RO(input_seen);

static ssize_t dpi_show(struct device *dev, struct device_attribute *attribute,
			char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	struct lorgar_shared_state *shared;
	unsigned long flags;
	u16 dpi;
	u8 stage;
	bool valid;

	if (!drv || !drv->ctx)
		return -ENODEV;
	shared = &drv->ctx->state;
	spin_lock_irqsave(&shared->lock, flags);
	stage = shared->active_dpi_stage;
	valid = shared->active_stage_valid && stage < LORGAR_DPI_STAGE_COUNT &&
		shared->stage_dpi_valid[stage];
	dpi = valid ? shared->stage_dpis[stage] : 0;
	spin_unlock_irqrestore(&shared->lock, flags);
	return valid ? sysfs_emit(buf, "%u\n", dpi) : -ENODATA;
}

static ssize_t dpi_store(struct device *dev, struct device_attribute *attribute,
			 const char *buf, size_t count)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	struct lorgar_shared_state *shared;
	unsigned long flags;
	u8 command[8] = { LORGAR_CMD_SET_DPI };
	u8 stage;
	u32 dpi;
	bool stale;
	int ret;

	if (!drv || !drv->ctx)
		return -ENODEV;
	shared = &drv->ctx->state;
	ret = kstrtou32(buf, 0, &dpi);
	if (ret < 0 || dpi < LORGAR_MIN_DPI || dpi > LORGAR_MAX_DPI)
		return -EINVAL;
	spin_lock_irqsave(&shared->lock, flags);
	if (!shared->active_stage_valid) {
		spin_unlock_irqrestore(&shared->lock, flags);
		return -ENODATA;
	}
	stage = shared->active_dpi_stage;
	spin_unlock_irqrestore(&shared->lock, flags);
	command[2] = stage;
	command[3] = dpi & 0xff;
	command[4] = (dpi >> 8) & 0xff;
	ret = lorgar_send_feature(drv, command, sizeof(command));
	if (ret < 0)
		return ret;
	spin_lock_irqsave(&shared->lock, flags);
	stale = !shared->active_stage_valid ||
		shared->active_dpi_stage != stage;
	if (stale) {
		shared->stage_dpi_valid[stage] = false;
	} else {
		shared->stage_dpis[stage] = dpi;
		shared->stage_dpi_valid[stage] = true;
	}
	spin_unlock_irqrestore(&shared->lock, flags);
	if (stale)
		lorgar_schedule_notification(drv->ctx, LORGAR_NOTIFY_DPI);
	return count;
}
static DEVICE_ATTR_RW(dpi);

static ssize_t dpi_stage_show(struct device *dev,
			      struct device_attribute *attribute, char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 stage;
	bool valid;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	valid = drv->ctx->state.active_stage_valid;
	stage = drv->ctx->state.active_dpi_stage;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return valid ? sysfs_emit(buf, "%u\n", stage) : -ENODATA;
}

static ssize_t dpi_stage_store(struct device *dev,
			       struct device_attribute *attribute,
			       const char *buf, size_t count)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 command[8] = { LORGAR_CMD_ACTIVE_STAGE };
	u8 stage;
	int ret;

	if (!drv || !drv->ctx)
		return -ENODEV;
	ret = kstrtou8(buf, 0, &stage);
	if (ret < 0 || stage >= LORGAR_DPI_STAGE_COUNT)
		return -EINVAL;
	lorgar_build_active_stage_report(command, stage);
	ret = lorgar_send_feature(drv, command, sizeof(command));
	if (ret < 0)
		return ret;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	drv->ctx->state.active_dpi_stage = stage;
	drv->ctx->state.active_stage_valid = true;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	lorgar_schedule_notification(drv->ctx, LORGAR_NOTIFY_DPI);
	return count;
}
static DEVICE_ATTR_RW(dpi_stage);

static ssize_t polling_rate_show(struct device *dev,
				 struct device_attribute *attribute, char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 interval;
	bool valid;
	unsigned int hz;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	valid = drv->ctx->state.polling_rate_valid;
	interval = drv->ctx->state.polling_rate_interval;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	if (!valid)
		return -ENODATA;
	switch (interval) {
	case 0x01:
		hz = 1000;
		break;
	case 0x02:
		hz = 500;
		break;
	case 0x04:
		hz = 250;
		break;
	case 0x08:
		hz = 125;
		break;
	default:
		return -ENODATA;
	}
	return sysfs_emit(buf, "%u\n", hz);
}

static ssize_t polling_rate_store(struct device *dev,
				  struct device_attribute *attribute,
				  const char *buf, size_t count)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 command[8] = { LORGAR_CMD_POLL_RATE };
	u8 interval;
	unsigned int hz;
	int ret;

	if (!drv || !drv->ctx)
		return -ENODEV;
	ret = kstrtouint(buf, 0, &hz);
	if (ret < 0)
		return -EINVAL;
	switch (hz) {
	case 1000:
		interval = 0x01;
		break;
	case 500:
		interval = 0x02;
		break;
	case 250:
		interval = 0x04;
		break;
	case 125:
		interval = 0x08;
		break;
	default:
		return -EINVAL;
	}
	command[3] = interval;
	ret = lorgar_send_feature(drv, command, sizeof(command));
	if (ret < 0)
		return ret;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	drv->ctx->state.polling_rate_interval = interval;
	drv->ctx->state.polling_rate_valid = true;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return count;
}
static DEVICE_ATTR_RW(polling_rate);

static ssize_t led_mode_show(struct device *dev,
			     struct device_attribute *attribute, char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 mode;
	bool valid;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	valid = drv->ctx->state.led_mode_valid;
	mode = drv->ctx->state.led_mode;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return valid ? sysfs_emit(buf, "%u\n", mode) : -ENODATA;
}

static ssize_t led_mode_store(struct device *dev,
			      struct device_attribute *attribute,
			      const char *buf, size_t count)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 command[8] = { LORGAR_CMD_LIGHT_MODE };
	u8 mode;
	int ret;

	if (!drv || !drv->ctx)
		return -ENODEV;
	ret = kstrtou8(buf, 0, &mode);
	if (ret < 0 || mode < 1 || mode > 5)
		return -EINVAL;
	command[3] = mode;
	command[4] = 0x80;
	command[5] = mode == 5 ? 0x01 : 0x05;
	ret = lorgar_send_feature(drv, command, sizeof(command));
	if (ret < 0)
		return ret;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	drv->ctx->state.led_mode = mode;
	drv->ctx->state.led_mode_valid = true;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return count;
}
static DEVICE_ATTR_RW(led_mode);

static ssize_t led_color_show(struct device *dev,
			      struct device_attribute *attribute, char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 red;
	u8 green;
	u8 blue;
	bool valid;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	valid = drv->ctx->state.led_color_valid;
	red = drv->ctx->state.led_r;
	green = drv->ctx->state.led_g;
	blue = drv->ctx->state.led_b;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return valid ? sysfs_emit(buf, "%02x%02x%02x\n", red, green, blue) :
		       -ENODATA;
}

static ssize_t led_color_store(struct device *dev,
			       struct device_attribute *attribute,
			       const char *buf, size_t count)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	u8 command[8] = { LORGAR_CMD_LIGHT_COLOR };
	u32 color;
	int ret;

	if (!drv || !drv->ctx)
		return -ENODEV;
	ret = kstrtou32(buf, 16, &color);
	if (ret < 0 || color > 0xffffff)
		return -EINVAL;
	command[3] = (color >> 16) & 0xff;
	command[4] = (color >> 8) & 0xff;
	command[5] = color & 0xff;
	ret = lorgar_send_feature(drv, command, sizeof(command));
	if (ret < 0)
		return ret;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	drv->ctx->state.led_r = command[3];
	drv->ctx->state.led_g = command[4];
	drv->ctx->state.led_b = command[5];
	drv->ctx->state.led_color_valid = true;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return count;
}
static DEVICE_ATTR_RW(led_color);

static ssize_t battery_indicator_show(struct device *dev,
				      struct device_attribute *attribute,
				      char *buf)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	bool enabled;

	if (!drv || !drv->ctx)
		return -ENODEV;
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	enabled = drv->ctx->state.battery_indicator_enabled;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	return sysfs_emit(buf, "%u\n", enabled);
}

static ssize_t battery_indicator_store(struct device *dev,
				       struct device_attribute *attribute,
				       const char *buf, size_t count)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(to_hid_device(dev));
	unsigned long flags;
	bool enabled;
	int ret;

	if (!drv || !drv->ctx)
		return -ENODEV;
	ret = kstrtobool(buf, &enabled);
	if (ret < 0)
		return ret;
	cancel_delayed_work_sync(&drv->indicator_work);
	spin_lock_irqsave(&drv->ctx->state.lock, flags);
	drv->ctx->state.battery_indicator_enabled = enabled;
	spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
	if (enabled) {
		drv->indicator_band_valid = false;
		mod_delayed_work(system_dfl_wq, &drv->indicator_work, 0);
	}
	return count;
}
static DEVICE_ATTR_RW(battery_indicator);

static struct attribute *lorgar_attrs[] = {
	&dev_attr_hardware_status.attr,
	&dev_attr_input_seen.attr,
	&dev_attr_dpi.attr,
	&dev_attr_dpi_stage.attr,
	&dev_attr_polling_rate.attr,
	&dev_attr_led_mode.attr,
	&dev_attr_led_color.attr,
	&dev_attr_battery_indicator.attr,
	NULL,
};

static const struct attribute_group lorgar_attr_group = {
	.attrs = lorgar_attrs,
};

static int lorgar_raw_event(struct hid_device *hdev, struct hid_report *report,
			    u8 *data, int size)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	struct lorgar_shared_state *shared;
	unsigned long flags;
	bool battery_changed;
	bool battery_notify;
	bool link_changed;
	bool stage_changed;
	bool physical_activity;
	bool input_changed;
	u8 capacity;
	u8 stage;

	if (!drv || !drv->ctx)
		return 0;
	shared = &drv->ctx->state;
	if (drv->is_config_interface &&
	    lorgar_parse_state_report(data, size, &capacity)) {
		spin_lock_irqsave(&shared->lock, flags);
		battery_changed = !shared->battery_valid ||
			shared->battery_capacity != capacity;
		battery_notify = battery_changed || !drv->battery;
		link_changed = shared->link_state != LORGAR_LINK_ONLINE;
		shared->battery_capacity = capacity;
		shared->battery_valid = true;
		shared->link_state = LORGAR_LINK_ONLINE;
		lorgar_note_response(shared);
		shared->query_pending = false;
		shared->next_query = jiffies +
			msecs_to_jiffies(LORGAR_QUERY_INTERVAL_MS);
		spin_unlock_irqrestore(&shared->lock, flags);
		if (battery_changed || link_changed)
			lorgar_kick_indicator(drv->ctx);
		if (battery_notify || link_changed)
			lorgar_schedule_notification(
				drv->ctx,
				(battery_notify ? LORGAR_NOTIFY_BATTERY : 0) |
				(link_changed ? LORGAR_NOTIFY_LINK : 0));
		return 0;
	}

	if (report && lorgar_parse_dpi_event(report->id, data, size, &stage)) {
		spin_lock_irqsave(&shared->lock, flags);
		stage_changed = !shared->active_stage_valid ||
			shared->active_dpi_stage != stage;
		shared->active_dpi_stage = stage;
		shared->active_stage_valid = true;
		input_changed = !shared->input_seen;
		shared->input_seen = true;
		spin_unlock_irqrestore(&shared->lock, flags);
		if (stage_changed || input_changed)
			lorgar_schedule_notification(
				drv->ctx,
				(stage_changed ? LORGAR_NOTIFY_DPI : 0) |
				(input_changed ? LORGAR_NOTIFY_INPUT : 0));
		return 0;
	}

	physical_activity = drv->interface_number == 0 && size > 0;
	if (physical_activity) {
		spin_lock_irqsave(&shared->lock, flags);
		input_changed = !shared->input_seen;
		shared->input_seen = true;
		spin_unlock_irqrestore(&shared->lock, flags);
		if (input_changed)
			lorgar_schedule_notification(drv->ctx,
						     LORGAR_NOTIFY_INPUT);
	}
	return 0;
}

static bool lorgar_is_physical_usb(struct hid_device *hdev)
{
	struct device *parent = hdev->dev.parent;
	const char *bus_name = parent ? dev_bus_name(parent) : NULL;

	return hdev->bus == BUS_USB && bus_name && !strcmp(bus_name, "usb");
}

static bool lorgar_match(struct hid_device *hdev, bool ignore_special_driver)
{
	return lorgar_is_physical_usb(hdev);
}

static int lorgar_probe(struct hid_device *hdev,
			const struct hid_device_id *id)
{
	struct lorgar_drvdata *drv;
	struct lorgar_device_ctx *ctx = NULL;
	struct usb_interface *interface;
	struct usb_device *udev = NULL;
	unsigned long flags;
	int ret;

	if (!lorgar_is_physical_usb(hdev))
		return -ENODEV;
	interface = to_usb_interface(hdev->dev.parent);
	if (!interface->cur_altsetting)
		return -ENODEV;

	ret = hid_parse(hdev);
	if (ret)
		return ret;
	ret = hid_hw_start(hdev, HID_CONNECT_DEFAULT);
	if (ret)
		return ret;
	udev = interface_to_usbdev(interface);
	if (udev)
		ctx = lorgar_get_or_create_ctx(udev);
	if (!ctx) {
		ret = -ENOMEM;
		goto err_stop;
	}

	drv = devm_kzalloc(&hdev->dev, sizeof(*drv), GFP_KERNEL);
	if (!drv) {
		ret = -ENOMEM;
		goto err_ctx;
	}
	drv->hdev = hdev;
	drv->ctx = ctx;
	drv->interface_number =
		interface->cur_altsetting->desc.bInterfaceNumber;
	drv->is_config_interface = drv->interface_number == 2;
	mutex_init(&drv->io_lock);
	INIT_WORK(&drv->notification_work, lorgar_notification_work);
	INIT_DELAYED_WORK(&drv->battery_work, lorgar_battery_work);
	INIT_DELAYED_WORK(&drv->indicator_work, lorgar_indicator_work);
	hid_set_drvdata(hdev, drv);
	if (!drv->is_config_interface)
		return 0;
	snprintf(drv->battery_name, sizeof(drv->battery_name),
		 "lorgar_mouse_battery_%s", dev_name(&hdev->dev));
	drv->battery_desc.name = drv->battery_name;
	drv->battery_desc.type = POWER_SUPPLY_TYPE_BATTERY;
	drv->battery_desc.properties = lorgar_battery_props;
	drv->battery_desc.num_properties = ARRAY_SIZE(lorgar_battery_props);
	drv->battery_desc.get_property = lorgar_battery_get_property;

	spin_lock_irqsave(&ctx->state.lock, flags);
	if (ctx->config_hdev) {
		spin_unlock_irqrestore(&ctx->state.lock, flags);
		ret = -EBUSY;
		goto err_ctx;
	}
	ctx->config_hdev = hdev;
	ctx->state.notifications = 0;
	spin_unlock_irqrestore(&ctx->state.lock, flags);
	ret = sysfs_create_group(&hdev->dev.kobj, &lorgar_attr_group);
	if (ret)
		goto err_config;

	lorgar_schedule_notification(ctx, LORGAR_NOTIFY_BATTERY);
	mod_delayed_work(system_dfl_wq, &drv->battery_work, 0);
	hid_info(hdev, "LORGAR MSA10W driver ready\n");
	return 0;

err_config:
	spin_lock_irqsave(&ctx->state.lock, flags);
	if (ctx->config_hdev == hdev)
		ctx->config_hdev = NULL;
	spin_unlock_irqrestore(&ctx->state.lock, flags);
	cancel_work_sync(&drv->notification_work);
err_ctx:
	lorgar_put_ctx(ctx);
err_stop:
	hid_hw_stop(hdev);
	return ret;
}

static void lorgar_remove(struct hid_device *hdev)
{
	struct lorgar_drvdata *drv = hid_get_drvdata(hdev);
	unsigned long flags;

	if (drv && drv->is_config_interface) {
		cancel_delayed_work_sync(&drv->battery_work);
		cancel_delayed_work_sync(&drv->indicator_work);
		spin_lock_irqsave(&drv->ctx->state.lock, flags);
		if (drv->ctx->config_hdev == hdev)
			drv->ctx->config_hdev = NULL;
		spin_unlock_irqrestore(&drv->ctx->state.lock, flags);
		cancel_work_sync(&drv->notification_work);
		sysfs_remove_group(&hdev->dev.kobj, &lorgar_attr_group);
	}
	if (drv && drv->ctx)
		lorgar_put_ctx(drv->ctx);
	hid_hw_stop(hdev);
}

static const struct hid_device_id lorgar_devices[] = {
	{ HID_USB_DEVICE(USB_VENDOR_ID_LORGAR,
			 USB_DEVICE_ID_LORGAR_MSA10W_WIRED) },
	{ HID_USB_DEVICE(USB_VENDOR_ID_LORGAR,
			 USB_DEVICE_ID_LORGAR_MSA10W_WIRELESS) },
	{ }
};
MODULE_DEVICE_TABLE(hid, lorgar_devices);

static struct hid_driver lorgar_driver = {
	.name = DRIVER_NAME,
	.id_table = lorgar_devices,
	.match = lorgar_match,
	.probe = lorgar_probe,
	.remove = lorgar_remove,
	.raw_event = lorgar_raw_event,
};
module_hid_driver(lorgar_driver);

MODULE_AUTHOR("Sqrilizz");
MODULE_DESCRIPTION("HID driver for LORGAR MSA10W Wireless/Wired Gaming Mouse");
MODULE_LICENSE("GPL");
MODULE_VERSION("2.3.4");
