"""
Example Python WebSocket client for Chainlink BTC Price Server.
Prints raw JSON messages with indent=4.

Usage:
    pip install websockets
    python client.py
"""

import asyncio
import json
import websockets

SERVER_URL = "ws://127.0.0.1:8080"

async def subscribe():
    async with websockets.connect(SERVER_URL) as websocket:
        # Subscribe with 1000 seconds history
        subscription = {
            "symbol": "btc",
            "history_seconds": 1000
        }
        await websocket.send(json.dumps(subscription))
        
        async for message in websocket:
            data = json.loads(message)
            print(json.dumps(data, indent=4))
            print()  # Blank line between messages

if __name__ == "__main__":
    try:
        asyncio.run(subscribe())
    except KeyboardInterrupt:
        pass
    except Exception as e:
        print(f"Error: {e}")
