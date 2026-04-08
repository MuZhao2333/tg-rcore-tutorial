// dummy.c - 空实现文件
// 用于替代需要特殊库（SDL/Allegro/网络等）的 C 文件

// 声音相关
void I_InitSound(void) {}
void I_ShutdownSound(void) {}
void I_UpdateSoundParams(void) {}
void I_StartSound(void) {}
void I_StopSound(void) {}
void I_SubmitSound(void) {}
void I_GetSfxName(void) {}
struct sfxinfo_t {};
void I_RegisterSong(void) {}
void I_PlaySong(void) {}
void I_PauseSong(void) {}
void I_ResumeSong(void) {}
void I_StopSong(void) {}
void I_DestroySong(void) {}
int I_QrySongPlaying(void) {}
void I_InitMusic(void) {}
void I_ShutdownMusic(void) {}
void I_PrecacheSounds(void) {}

// CD 音乐
void I_ToggleCD(void) {}
void I_PlayCD(void) {}
void I_StopCD(void) {}
void I_PauseCD(void) {}
void I_SetVolumeCD(void) {}
void I_EjectCD(void) {}
void I_CloseCD(void) {}
