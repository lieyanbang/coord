import { createServer } from 'node:http';

// WHATWG Fetch rejects these ports even when a local listener can bind them.
const fetchBlockedPorts = new Set([
  0, 1, 7, 9, 11, 13, 15, 17, 19, 20, 21, 22, 23, 25, 37, 42, 43, 53, 69,
  77, 79, 87, 95, 101, 102, 103, 104, 109, 110, 111, 113, 115, 117, 119,
  123, 135, 137, 139, 143, 179, 389, 427, 465, 512, 513, 514, 515, 526,
  530, 531, 532, 540, 548, 554, 556, 563, 587, 601, 636, 989, 990, 993,
  995, 1719, 1720, 1723, 2049, 3659, 4045, 5060, 5061, 6000, 6566, 6665,
  6666, 6667, 6668, 6669, 6697, 10080,
]);

export function isFetchBlockedPort(port) {
  return fetchBlockedPorts.has(port);
}

export function listen(server, port = 0, host = '127.0.0.1') {
  return new Promise((resolve, reject) => {
    const bind = () => {
      const onError = error => {
        server.off('listening', onListening);
        reject(error);
      };
      const onListening = () => {
        server.off('error', onError);
        const address = server.address();
        if (port === 0 && isFetchBlockedPort(address.port)) {
          server.close(error => error ? reject(error) : bind());
          return;
        }
        resolve(address);
      };
      server.once('error', onError);
      server.once('listening', onListening);
      server.listen(port, host);
    };
    bind();
  });
}

export async function reservePort(host = '127.0.0.1') {
  const server = createServer();
  const address = await listen(server, 0, host);
  await new Promise((resolve, reject) => {
    server.close(error => error ? reject(error) : resolve());
  });
  return address.port;
}
