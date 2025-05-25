# Smart Station Website

A Rust web server built with Axum that handles IoT device communication, data storage, and real-time music control via WebSocket and TCP connections.

## Features

- **TCP Server**: Listens on port 6000 for IoT device commands
- **WebSocket Server**: Real-time communication with web clients
- **REST API**: Store and retrieve clock data and sensor readings
- **MongoDB Integration**: Persistent data storage
- **Music Control**: Translates device commands to music actions

## API Endpoints

### REST API
- `POST /api/hello` - Simple greeting endpoint
- `POST /api/clock` - Store clock data with binary time representation
- `GET /api/clock` - Retrieve recent clock data (last 10 entries)
- `POST /api/sensor` - Store sensor readings
- `GET /api/sensor` - Retrieve recent sensor readings (last 10 entries)

### WebSocket
- `GET /ws` - WebSocket connection for real-time updates

## Command Protocol

The TCP server accepts JSON commands with the following format:

```json
{
  "type": "command",
  "id": "device_id",
  "payload": {
    "command": "up|down|left|right|press"
  }
}
```

### Command Mapping
- `up` → Volume up
- `down` → Volume down  
- `left` → Previous track
- `right` → Next track
- `press` → Play/pause

## Setup

1. **Prerequisites**
   - Rust (latest stable)
   - MongoDB running on `localhost:27017`

2. **Run the server**
   ```bash
   cargo run
   ```

3. **Server addresses**
   - HTTP/WebSocket: `http://127.0.0.1:3000`
   - TCP: `192.168.4.8:6000`

## Architecture

- **HTTP Server**: Handles web interface and REST API
- **TCP Server**: Receives commands from IoT devices
- **WebSocket Hub**: Broadcasts music control commands to connected clients
- **MongoDB**: Stores clock data and sensor readings with timestamps

The server bridges IoT devices and web clients, allowing physical device interactions to control music playback in real-time.
