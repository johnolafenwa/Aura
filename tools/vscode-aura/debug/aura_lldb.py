"""lldb formatters for programs built with `aura build --debug`.

A debug build describes every runtime value (`str`, `list[T]`, a class
instance, and so on) as the record `AuraValue`, whose one field holds the
value's address, under a typedef of its Aura type name. This module shows
such a value as the program's runtime renders it, by calling
`aura_debug_render` in the stopped program. Load it with `command script import /path/to/aura_lldb.py`; the
Aura extension for VS Code loads it for every Aura debug session.
"""

import lldb

RENDER = "((const char *(*)(void *)){function})((void *){address})"
TIMEOUT_MICROSECONDS = 500_000


def render_function(target):
    """The load address of `aura_debug_render`, called by address because
    the runtime's own debug info also names it."""
    for context in target.FindSymbols("aura_debug_render", lldb.eSymbolTypeCode):
        symbol = context.GetSymbol()
        if symbol.IsValid() and symbol.GetName() == "aura_debug_render":
            address = symbol.GetStartAddress().GetLoadAddress(target)
            if address != lldb.LLDB_INVALID_ADDRESS:
                return address
    return None


def render(value, _internal_dict):
    """The summary of one runtime value."""
    record = value.GetNonSyntheticValue()
    address = record.GetChildMemberWithName("address").GetValueAsUnsigned(0)
    if address == 0:
        return "<unset>"
    frame = value.GetFrame()
    if not frame.IsValid():
        return "<unavailable>"
    function = render_function(value.GetTarget())
    if function is None:
        return "<unavailable>"
    options = lldb.SBExpressionOptions()
    options.SetTimeoutInMicroSeconds(TIMEOUT_MICROSECONDS)
    options.SetTryAllThreads(False)
    options.SetUnwindOnError(True)
    options.SetIgnoreBreakpoints(True)
    options.SetLanguage(lldb.eLanguageTypeC)
    result = frame.EvaluateExpression(RENDER.format(function=function, address=address), options)
    if not result.GetError().Success():
        return "<unavailable>"
    text = result.GetSummary()
    if text is None:
        return "<unavailable>"
    # lldb quotes a C string; the runtime already quoted a `str`.
    if len(text) >= 2 and text[0] == '"' and text[-1] == '"':
        text = text[1:-1].encode("utf-8").decode("unicode_escape").encode("latin-1").decode("utf-8")
    return text


class NoChildren:
    """Shows a runtime value as its rendering alone, not as a record."""

    def __init__(self, value, _internal_dict):
        self.value = value

    def num_children(self):
        return 0

    def get_child_index(self, _name):
        return -1

    def get_child_at_index(self, _index):
        return None

    def update(self):
        return False

    def has_children(self):
        return False


def __lldb_init_module(debugger, _internal_dict):
    debugger.HandleCommand(
        "type summary add --category aura --python-function aura_lldb.render AuraValue"
    )
    debugger.HandleCommand(
        "type synthetic add --category aura --python-class aura_lldb.NoChildren AuraValue"
    )
    debugger.HandleCommand("type category enable aura")
