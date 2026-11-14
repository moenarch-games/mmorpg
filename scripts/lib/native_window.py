"""X11 pointer/clipboard operations for the existing opt-in native journey.

Only used on its privately owned Xvfb display. No user display or clipboard is touched.
"""
import ctypes as C
import os
import time
from PIL import ImageGrab

class Pointer:

    def __init__(self, display):
        self.display = display
        self.x = C.CDLL('libX11.so.6')
        self.t = C.CDLL('libXtst.so.6')
        self.x.XOpenDisplay.argtypes = [C.c_char_p]
        self.x.XOpenDisplay.restype = C.c_void_p
        self.d = self.x.XOpenDisplay(display.encode())
        if not self.d:
            raise RuntimeError('cannot open isolated display')
        D, W = (C.c_void_p, C.c_ulong)
        self.x.XDefaultRootWindow.argtypes = [D]
        self.x.XDefaultRootWindow.restype = W
        self.root = self.x.XDefaultRootWindow(self.d)
        self.x.XQueryTree.argtypes = [D, W, C.POINTER(W), C.POINTER(W), C.POINTER(C.POINTER(W)), C.POINTER(C.c_uint)]
        self.x.XFetchName.argtypes = [D, W, C.POINTER(C.c_char_p)]
        self.x.XFree.argtypes = [D]
        self.x.XSetInputFocus.argtypes = [D, W, C.c_int, W]
        self.x.XRaiseWindow.argtypes = [D, W]
        self.x.XFlush.argtypes = [D]
        self.x.XSync.argtypes = [D, C.c_int]
        self.x.XStringToKeysym.argtypes = [C.c_char_p]
        self.x.XStringToKeysym.restype = W
        self.x.XKeysymToKeycode.argtypes = [D, W]
        self.x.XKeysymToKeycode.restype = C.c_uint
        self.t.XTestFakeMotionEvent.argtypes = [D, C.c_int, C.c_int, C.c_int, W]
        self.t.XTestFakeButtonEvent.argtypes = [D, C.c_uint, C.c_int, W]
        self.t.XTestFakeKeyEvent.argtypes = [D, C.c_uint, C.c_int, W]

    def windows(self):
        root, parent = (C.c_ulong(), C.c_ulong())
        children = C.POINTER(C.c_ulong)()
        count = C.c_uint()
        self.x.XQueryTree(self.d, self.root, C.byref(root), C.byref(parent), C.byref(children), C.byref(count))
        result = []
        for i in range(count.value):
            name = C.c_char_p()
            self.x.XFetchName(self.d, children[i], C.byref(name))
            if name.value:
                result.append((children[i], name.value.decode(errors='replace')))
                self.x.XFree(name)
        if children:
            self.x.XFree(children)
        return result

    def focus(self, window):
        self.x.XRaiseWindow(self.d, window)
        self.x.XSetInputFocus(self.d, window, 1, 0)
        self.x.XSync(self.d, 0)

    def click(self, x, y):
        self.t.XTestFakeMotionEvent(self.d, -1, x, y, 0)
        self.x.XSync(self.d, 0)
        self.t.XTestFakeButtonEvent(self.d, 1, 1, 0)
        self.x.XSync(self.d, 0)
        time.sleep(0.08)
        self.t.XTestFakeButtonEvent(self.d, 1, 0, 0)
        self.x.XSync(self.d, 0)

    def key(self, name, down=True):
        code = self.x.XKeysymToKeycode(self.d, self.x.XStringToKeysym(name.encode()))
        self.t.XTestFakeKeyEvent(self.d, code, int(down), 0)
        self.x.XSync(self.d, 0)

    def tap(self, name):
        self.key(name)
        time.sleep(0.05)
        self.key(name, False)
        time.sleep(0.05)

    def capture(self, path):
        ImageGrab.grab(xdisplay=self.display).save(path)

    def close(self, window):
        W = C.c_ulong
        D = C.c_void_p

        class Data(C.Union):
            _fields_ = [('words', C.c_long * 5), ('bytes', C.c_char * 20)]

        class Event(C.Structure):
            _fields_ = [('type', C.c_int), ('serial', W), ('send_event', C.c_int), ('display', D), ('window', W), ('message_type', W), ('format', C.c_int), ('data', Data)]
        self.x.XInternAtom.argtypes = [D, C.c_char_p, C.c_int]
        self.x.XInternAtom.restype = W
        self.x.XSendEvent.argtypes = [D, W, C.c_int, C.c_long, D]
        self.x.XSync.argtypes = [D, C.c_int]
        event = Event()
        event.type = 33
        event.display = self.d
        event.window = window
        event.format = 32
        event.message_type = self.x.XInternAtom(self.d, b'WM_PROTOCOLS', 0)
        event.data.words[0] = self.x.XInternAtom(self.d, b'WM_DELETE_WINDOW', 0)
        self.x.XSendEvent(self.d, window, 0, 0, C.byref(event))
        self.x.XSync(self.d, 0)

    def clipboard(self):
        """Read the actual OS CLIPBOARD selection, without seeding or bypassing Copy."""
        W, D = (C.c_ulong, C.c_void_p)
        x = self.x
        x.XCreateSimpleWindow.argtypes = [D, W, C.c_int, C.c_int, C.c_uint, C.c_uint, C.c_uint, W, W]
        x.XCreateSimpleWindow.restype = W
        x.XInternAtom.argtypes = [D, C.c_char_p, C.c_int]
        x.XInternAtom.restype = W
        x.XConvertSelection.argtypes = [D, W, W, W, W, W]
        x.XPending.argtypes = [D]
        x.XPending.restype = C.c_int
        x.XNextEvent.argtypes = [D, D]
        x.XGetWindowProperty.argtypes = [D, W, W, C.c_long, C.c_long, C.c_int, W, C.POINTER(W), C.POINTER(C.c_int), C.POINTER(W), C.POINTER(W), C.POINTER(C.POINTER(C.c_ubyte))]
        x.XDestroyWindow.argtypes = [D, W]
        window = x.XCreateSimpleWindow(self.d, self.root, 0, 0, 1, 1, 0, 0, 0)
        try:
            prop = x.XInternAtom(self.d, b'MMORPG_SMOKE_CLIPBOARD', 0)
            x.XConvertSelection(self.d, x.XInternAtom(self.d, b'CLIPBOARD', 0), x.XInternAtom(self.d, b'UTF8_STRING', 0), prop, window, 0)
            x.XFlush(self.d)
            deadline = time.monotonic() + 5
            while True:
                if time.monotonic() > deadline:
                    raise TimeoutError('native Copy invitation did not supply the OS clipboard')
                if not x.XPending(self.d):
                    time.sleep(0.02)
                    continue
                event = C.create_string_buffer(192)
                x.XNextEvent(self.d, event)
                if C.cast(event, C.POINTER(C.c_int)).contents.value == 31:
                    break
            actual = W()
            fmt = C.c_int()
            count = W()
            after = W()
            data = C.POINTER(C.c_ubyte)()
            x.XGetWindowProperty(self.d, window, prop, 0, 8192, 0, 0, C.byref(actual), C.byref(fmt), C.byref(count), C.byref(after), C.byref(data))
            if not data:
                return None
            text = C.string_at(data, count.value).decode()
            x.XFree(data)
            return text
        finally:
            x.XDestroyWindow(self.d, window)

    def paste(self):
        self.key('Control_L')
        self.tap('a')
        self.tap('v')
        self.key('Control_L', False)
