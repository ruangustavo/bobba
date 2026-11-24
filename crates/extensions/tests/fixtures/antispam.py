import sys

from g_python.gextension import Extension
from g_python.hmessage import Direction

extension = Extension(
    {"title": "AntiSpam", "description": "Bobba test fixture", "version": "1.0", "author": "bobba"},
    sys.argv,
)
extension.start()


def on_chat(message):
    text = message.packet.read_string()
    if "spam" in text:
        message.is_blocked = True
    if text == "!ping":
        message.is_blocked = True
        extension.send_to_client('{in:Chat}{i:7}{s:"pong"}{i:0}{i:0}{i:-1}')


extension.intercept(Direction.TO_SERVER, on_chat, "Chat")
