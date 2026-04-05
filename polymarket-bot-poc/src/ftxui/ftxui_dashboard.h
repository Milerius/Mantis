#pragma once

#ifdef __cplusplus
extern "C" {
#endif

typedef struct FtxuiDashboard FtxuiDashboard;

FtxuiDashboard* dashboard_create(void);
void dashboard_destroy(FtxuiDashboard* d);

// Renders one frame. Returns key pressed (0 if none).
char dashboard_render(FtxuiDashboard* d, const void* snapshot_ptr);

#ifdef __cplusplus
}
#endif
