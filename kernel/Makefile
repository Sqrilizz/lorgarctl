obj-m += hid-lorgar.o

KERNELRELEASE ?= $(shell uname -r)
KDIR ?= /lib/modules/$(KERNELRELEASE)/build
PWD := $(shell pwd)
HOSTCC ?= cc
LLVM ?= $(if $(shell grep -sh '^CONFIG_CC_IS_CLANG=y' $(KDIR)/.config $(KDIR)/include/config/auto.conf),1,)
MODDIR ?= /lib/modules/$(KERNELRELEASE)/extra

.PHONY: default install dkms-install uninstall check live-check clean

default:
	$(MAKE) -C $(KDIR) M=$(PWD) LLVM=$(LLVM) modules

install: default
	install -D -m 0644 hid-lorgar.ko $(MODDIR)/hid-lorgar.ko
	depmod -a $(KERNELRELEASE)
	@echo "Module hid-lorgar installed to $(MODDIR) and depmod updated."

dkms-install:
	./install-dkms.sh

uninstall:
	rm -f $(MODDIR)/hid-lorgar.ko
	depmod -a $(KERNELRELEASE)
	@echo "Module hid-lorgar uninstalled."

check:
	bash -n reload.sh
	bash -n verify-live.sh
	bash -n install-dkms.sh
	$(MAKE) clean
	$(MAKE) LLVM=$(LLVM) W=1
	$(MAKE) protocol-test
	./protocol-test
	modinfo ./hid-lorgar.ko >/dev/null

protocol-test: tests/protocol_test.c hid-lorgar-protocol.h
	$(HOSTCC) -std=c11 -Wall -Wextra -Werror -pedantic tests/protocol_test.c -o $@

live-check:
	./verify-live.sh

clean:
	$(MAKE) -C $(KDIR) M=$(PWD) clean
	rm -f protocol-test
