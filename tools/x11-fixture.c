#include <X11/Xlib.h>
#include <X11/Xatom.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>
#include <signal.h>

static volatile sig_atomic_t action = 0;
static void request_change(int signal_number) {
    action = signal_number == SIGUSR1 ? 1 : signal_number == SIGUSR2 ? 2 : 3;
}

int main(void) {
    Display *display = XOpenDisplay(NULL);
    if (!display) return 1;
    Window root = DefaultRootWindow(display);
    Window window = XCreateSimpleWindow(display, root, 0, 0, 100, 100, 0, 0, 0);
    unsigned long pid = getpid();
    char hostname[256] = {0};
    if (gethostname(hostname, sizeof(hostname) - 1) != 0) return 2;
    XChangeProperty(display, root, XInternAtom(display, "_NET_ACTIVE_WINDOW", False), XA_WINDOW, 32, PropModeReplace, (unsigned char *)&window, 1);
    XChangeProperty(display, window, XInternAtom(display, "_NET_WM_PID", False), XA_CARDINAL, 32, PropModeReplace, (unsigned char *)&pid, 1);
    XChangeProperty(display, window, XA_WM_CLIENT_MACHINE, XA_STRING, 8, PropModeReplace, (unsigned char *)hostname, strlen(hostname));
    XMapWindow(display, window);
    XSync(display, False);
    printf("%lu\n", pid);
    fflush(stdout);
    signal(SIGUSR1, request_change);
    signal(SIGUSR2, request_change);
    signal(SIGTERM, request_change);
    while (action != 3) {
        if (action == 1 || action == 2) {
            Window active = action == 1 ? None : window;
            XChangeProperty(display, root, XInternAtom(display, "_NET_ACTIVE_WINDOW", False), XA_WINDOW, 32, PropModeReplace, (unsigned char *)&active, 1);
            XSync(display, False);
            action = 0;
        }
        pause();
    }
    XCloseDisplay(display);
    return 0;
}
