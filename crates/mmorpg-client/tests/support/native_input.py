"""Send real X11 pointer/key events to the focused winit window.

XTest is used only as an OS event source. No application result is fabricated.
"""
import ctypes
import ctypes.util
import sys
import signal
signal.alarm(10)

x11 = ctypes.CDLL(ctypes.util.find_library("X11"))
xtst = ctypes.CDLL(ctypes.util.find_library("Xtst"))
x11.XOpenDisplay.argtypes = [ctypes.c_char_p]
x11.XOpenDisplay.restype = ctypes.c_void_p
x11.XStringToKeysym.argtypes = [ctypes.c_char_p]
x11.XStringToKeysym.restype = ctypes.c_ulong
x11.XKeysymToKeycode.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XKeysymToKeycode.restype = ctypes.c_uint
x11.XFlush.argtypes = [ctypes.c_void_p]
x11.XCloseDisplay.argtypes = [ctypes.c_void_p]
x11.XRaiseWindow.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
x11.XSetInputFocus.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
x11.XSync.argtypes = [ctypes.c_void_p, ctypes.c_int]
x11.XDefaultRootWindow.argtypes = [ctypes.c_void_p]
x11.XDefaultRootWindow.restype = ctypes.c_ulong
x11.XTranslateCoordinates.argtypes = [ctypes.c_void_p, ctypes.c_ulong, ctypes.c_ulong, ctypes.c_int, ctypes.c_int, ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_int), ctypes.POINTER(ctypes.c_ulong)]
xtst.XTestFakeKeyEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeMotionEvent.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int, ctypes.c_int, ctypes.c_ulong]
xtst.XTestFakeButtonEvent.argtypes = [ctypes.c_void_p, ctypes.c_uint, ctypes.c_int, ctypes.c_ulong]
display = x11.XOpenDisplay(None)
if not display:
    raise RuntimeError("XTest acceptance requires a working X11 DISPLAY")
try:
    if sys.argv[1] == "focus":
        window = int(sys.argv[2])
        x11.XRaiseWindow(display, window)
        x11.XSetInputFocus(display, window, 2, 0)
    elif sys.argv[1] == "move":
        x, y, child = ctypes.c_int(), ctypes.c_int(), ctypes.c_ulong()
        if not x11.XTranslateCoordinates(display, int(sys.argv[2]), x11.XDefaultRootWindow(display),
                int(sys.argv[3]), int(sys.argv[4]), ctypes.byref(x), ctypes.byref(y), ctypes.byref(child)):
            raise RuntimeError("could not translate actual window coordinates")
        if not xtst.XTestFakeMotionEvent(display, -1, x.value, y.value, 0):
            raise RuntimeError("XTest could not enqueue pointer motion")
    elif sys.argv[1] in ("press", "release"):
        if not xtst.XTestFakeButtonEvent(display, 1, int(sys.argv[1] == "press"), 0):
            raise RuntimeError("XTest could not enqueue pointer button")
    elif sys.argv[1] == "key":
        code = x11.XKeysymToKeycode(display, x11.XStringToKeysym(sys.argv[2].encode()))
        if not code:
            raise ValueError("unknown X11 keysym")
        for pressed in (1, 0):
            if not xtst.XTestFakeKeyEvent(display, code, pressed, 0):
                raise RuntimeError("XTest could not enqueue keyboard event")
    else:
        raise ValueError("unknown XTest operation")
    x11.XSync(display, 0)
finally:
    x11.XCloseDisplay(display)
