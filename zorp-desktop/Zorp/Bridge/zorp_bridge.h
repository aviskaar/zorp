#ifndef ZORP_BRIDGE_H
#define ZORP_BRIDGE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Repairs the process environment PATH by reading from the user's login shell.
/// Returns 0 on success, or -1 if the login shell could not be evaluated.
int32_t zorp_bridge_repair_path(void);

/// Starts the in-process Tokio background server running zorp-web.
/// Attempts to bind requested_port (typically 7777). If occupied, binds ephemeral port 0.
/// Delivers the successfully bound port into out_port.
/// Returns 0 on success, or -1 on fatal failure.
int32_t zorp_bridge_start_server(uint16_t requested_port, const char *resource_dir, uint16_t *out_port);

/// Gracefully signals server cancellation and stops the Tokio runtime.
void zorp_bridge_stop_server(void);

#ifdef __cplusplus
}
#endif

#endif /* ZORP_BRIDGE_H */
