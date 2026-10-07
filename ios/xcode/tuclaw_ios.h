#ifndef TUCLAW_IOS_H
#define TUCLAW_IOS_H

#include <stdbool.h>

void tuclaw_ios_start(void);
void *gpui_ios_get_window(void);
bool gpui_ios_request_frame(void *window);
void gpui_ios_set_frame_waker(void *window, void (*waker)(void *context), void *context);
void gpui_ios_will_enter_foreground(void *app);
void gpui_ios_did_become_active(void *app);
void gpui_ios_will_resign_active(void *app);
void gpui_ios_did_enter_background(void *app);
void gpui_ios_will_terminate(void *app);
void gpui_ios_handle_open_url(void *url);

#endif
