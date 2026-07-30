#ifndef VAAPI_IMPORT_PROBE_H
#define VAAPI_IMPORT_PROBE_H

// Smoke test: returns 42. Confirms the probe C object is compiled and linked.
int vaapi_probe_selftest(void);

// Encode ONE H264 frame from a single-plane DRM-PRIME dmabuf, testing whether
// VAAPI can import the buffer with zero CPU round-trip. The dmabuf is wrapped as
// an AVDRMFrameDescriptor and pushed through:
//   buffer(DRM_PRIME) -> hwmap=derive_device=vaapi -> scale_vaapi=nv12 -> h264_vaapi
// Returns the encoded byte count on success, or a negative value on failure;
// `reason` (capacity `reason_len`) receives a human-readable failure string.
int vaapi_probe_encode_dmabuf(
    int dmabuf_fd,
    unsigned int width,
    unsigned int height,
    unsigned int drm_fourcc,
    unsigned long long drm_modifier,
    unsigned int stride,
    unsigned int offset,
    char* reason,
    int reason_len);

#endif
