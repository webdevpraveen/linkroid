// SPDX-License-Identifier: GPL-3.0-or-later
package com.webdevpraveen.linkroid.core

import android.app.ActivityOptions
import android.app.PendingIntent
import android.app.RemoteInput
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import com.webdevpraveen.linkroid.calls.PhoneCalls
import com.webdevpraveen.linkroid.camera.ContinuityCameraRequests
import com.webdevpraveen.linkroid.clipboard.PhoneClipboard
import com.webdevpraveen.linkroid.contacts.PhoneContacts
import com.webdevpraveen.linkroid.notifications.NotificationListener
import com.webdevpraveen.linkroid.media.PhoneMedia
import com.webdevpraveen.linkroid.photos.RecentPhotos
import com.webdevpraveen.linkroid.elevated.Elevated
import com.webdevpraveen.linkroid.mirror.AppWindows
import com.webdevpraveen.linkroid.mirror.InputService
import com.webdevpraveen.linkroid.mirror.MirrorRequest
import com.webdevpraveen.linkroid.mirror.MirrorRequests
import com.webdevpraveen.linkroid.mirror.MirrorService
import com.webdevpraveen.linkroid.sms.PhoneSms
import com.webdevpraveen.linkroid.storage.PhoneStorage
import com.webdevpraveen.linkroid.toggles.PhoneToggles
import com.webdevpraveen.linkroid.webcam.WebcamRequest
import com.webdevpraveen.linkroid.webcam.WebcamRequests
import com.webdevpraveen.linkroid.webcam.WebcamService
import com.webdevpraveen.linkroid.core.CallCommand
import com.webdevpraveen.linkroid.core.CallLogEntry
import com.webdevpraveen.linkroid.core.Contact
import com.webdevpraveen.linkroid.core.SmsAttachment
import com.webdevpraveen.linkroid.core.SmsMessage
import com.webdevpraveen.linkroid.core.SmsPartData
import com.webdevpraveen.linkroid.core.SmsThread
import com.webdevpraveen.linkroid.core.FileToSend
import com.webdevpraveen.linkroid.core.MediaAction
import com.webdevpraveen.linkroid.core.MirrorInputEvent
import com.webdevpraveen.linkroid.core.MirrorOptions
import com.webdevpraveen.linkroid.core.PhoneApp
import com.webdevpraveen.linkroid.core.PhoneToggleValue
import com.webdevpraveen.linkroid.core.PhotoAlbum
import com.webdevpraveen.linkroid.core.PhotoItem
import com.webdevpraveen.linkroid.core.PhotoThumb
import com.webdevpraveen.linkroid.core.NotificationFailure
import com.webdevpraveen.linkroid.core.Platform
import com.webdevpraveen.linkroid.core.StorageEntry
import com.webdevpraveen.linkroid.core.StorageReadFile
import com.webdevpraveen.linkroid.core.StorageWriteDone
import com.webdevpraveen.linkroid.core.TaskNotify
import com.webdevpraveen.linkroid.core.WebcamOptions

/**
 * What the core asks of the phone: ring it, and act on its notifications
 * for a PC (dismiss, reply, run an action).
 */
internal class PhonePlatform(
    context: Context,
    private val ringer: Ringer,
    private val media: PhoneMedia,
    /** Shows a link a PC sent: (PC's ID, link) → shown. */
    private val onLink: (String, String) -> Boolean,
    /** Shows a PC's Play on phone offer: (PC's ID, player, title) → shown. */
    private val onWatch: (String, String, String?, com.webdevpraveen.linkroid.core.WatchFileOffer?) -> Boolean,
    /** Asks the user to start the microphone for a PC: PC's ID → asked. */
    private val onMic: (String) -> Boolean,
    /** The PC stopped asking for (or listening to) the microphone: PC's ID. */
    private val onMicStop: (String) -> Unit,
    /** Shows or updates a watched task from a PC: (PC's ID, task) → shown. */
    private val onTask: (String, TaskNotify) -> Boolean,
    /** The phone's calls (created after this). */
    private val calls: () -> PhoneCalls,
    /** The phone's contacts (created after this). */
    private val contacts: () -> PhoneContacts,
    /** The phone's texts (created after this). */
    private val sms: () -> PhoneSms,
    /** The phone's quick settings (created after this). */
    private val toggles: () -> PhoneToggles,
    /** The phone's shared storage (created after this). */
    private val storage: () -> PhoneStorage,
    /** Asks the user to share the screen with a PC: (PC's ID, request) → asked. */
    private val onMirror: (String, MirrorRequest) -> Boolean,
    /** Asks the user to stream the camera as a webcam to a PC: (PC's ID, request) → asked. */
    private val onWebcam: (String, WebcamRequest) -> Boolean,
    /** Apps in windows of their own on PCs (Elevated). */
    private val appWindows: AppWindows,
    /** Resolves a paired PC's display name by ID. */
    private val nameOf: (String) -> String = { "" },
    /** Notifies when a PC starts or stops typing on this phone without mirroring. */
    private val onKeyboardFromPc: (String, Boolean) -> Unit = { _, _ -> },
) : Platform {
    private val context = context.applicationContext

    override fun startRinging() = ringer.startRinging()

    override fun stopRinging() = ringer.stopRinging()

    override fun setClipboard(text: String): Boolean = PhoneClipboard.write(context, text)

    override fun setClipboardPrivate(text: String): Boolean = PhoneClipboard.writeSensitive(context, text)

    override fun clearClipboardPrivate(text: String) = PhoneClipboard.clearSensitive(context, text)

    override fun setClipboardRich(text: String, html: String?, rtf: String?): Boolean =
        PhoneClipboard.write(context, text, html)

    override fun setClipboardImage(mime: String, bytes: ByteArray): Boolean =
        PhoneClipboard.writeImage(context, mime, bytes)

    override fun mediaCommand(player: String, action: MediaAction, position: ULong?) =
        media.command(player, action, position)

    override fun openLink(fromId: String, url: String): Boolean = onLink(fromId, url)

    override fun watchOffered(
        fromId: String,
        player: String,
        title: String?,
        file: com.webdevpraveen.linkroid.core.WatchFileOffer?,
    ): Boolean = onWatch(fromId, player, title, file)

    override fun taskNotify(fromId: String, task: TaskNotify): Boolean = onTask(fromId, task)

    override fun openPhoto(id: String): FileToSend? = RecentPhotos.open(context, id)

    override fun photoAlbums(): List<PhotoAlbum> = RecentPhotos.albums(context)

    override fun photoList(album: String?, before: Long?, beforeId: String, limit: UInt): List<PhotoItem> =
        RecentPhotos.list(context, album, before, beforeId, limit.toInt())

    override fun photoDelete(ids: List<String>): UInt = RecentPhotos.moveToTrash(context, ids).toUInt()

    override fun photoThumbs(ids: List<String>): List<PhotoThumb> = RecentPhotos.thumbs(context, ids)

    override fun callCommand(id: String, command: CallCommand): Boolean = calls().command(id, command)

    override fun callLog(before: Long?, limit: UInt): List<CallLogEntry> =
        calls().callLog(before, limit.toInt())

    override fun callDial(number: String): Boolean = calls().dial(number)

    override fun callSims(): List<com.webdevpraveen.linkroid.core.CallSim> = calls().sims()

    override fun callDialOn(number: String, sim: String): Boolean = calls().dialOn(number, sim)

    override fun contacts(query: String?, offset: UInt, limit: UInt): List<Contact> =
        contacts().list(query, offset.toInt(), limit.toInt())

    override fun mirrorRequested(pcId: String, options: MirrorOptions): Boolean =
        if (options.app != null) {
            appWindows.open(pcId, options)
        } else {
            // Kept within what the encoder can take: a zero or huge value from
            // the PC would only make this mirroring fail to start.
            val request = MirrorRequest(
                pcId,
                options.maxSize.coerceIn(320u, 4096u).toInt(),
                options.fps.coerceIn(1u, 120u).toInt(),
                options.bitrate.coerceIn(500_000u, 50_000_000u).toInt(),
                options.audio,
                options.stayAwake,
                options.screenOff,
                options.soundOnly,
            )
            if (Elevated.running && MirrorService.startElevated(context, request)) {
                true
            } else {
                onMirror(pcId, request)
            }
        }

    override fun mirrorPowerRequested(pcId: String, stayAwake: Boolean, screenOff: Boolean) {
        MirrorService.setPower(pcId, stayAwake, screenOff)
        appWindows.setPower(pcId, stayAwake, screenOff)
    }

    override fun mirrorSoundRequested(pcId: String, on: Boolean) = MirrorService.setSound(pcId, on)

    override fun mirrorStopRequested(pcId: String, session: UInt) {
        if (session != SCREEN) return appWindows.close(pcId, session)
        onKeyboardFromPc(pcId, false)
        MirrorRequests.dismiss(context, pcId)
        MirrorService.stop(pcId)
    }

    override fun mirrorKeyframeRequested(pcId: String, session: UInt) =
        if (session == SCREEN) MirrorService.keyframe(pcId) else appWindows.keyframe(pcId, session)

    override fun mirrorResizeRequested(pcId: String, session: UInt, width: UInt, height: UInt) {
        if (session != SCREEN) appWindows.resize(pcId, session, width.toInt(), height.toInt())
    }

    // Real events when Elevated runs; gestures through the accessibility service otherwise.
    override fun mirrorInput(pcId: String, session: UInt, requested: MirrorInputEvent) {
        // Send needs the accessibility service. Without it, it's a plain Enter.
        var input = requested
        if (requested is MirrorInputEvent.Key && requested.key == "send") {
            if (InputService.pressSend()) return
            input = MirrorInputEvent.Key("enter")
        }
        if (session != SCREEN) return appWindows.input(pcId, session, input)
        if (input is MirrorInputEvent.Key) {
            when (input.key) {
                "keyboard_on" -> {
                    onKeyboardFromPc(pcId, true)
                    return
                }
                "keyboard_off" -> {
                    onKeyboardFromPc(pcId, false)
                    return
                }
            }
        }
        if (input is MirrorInputEvent.Text && input.text.any { it.code > 127 } && InputService.running) {
            InputService.handle(input)
            return
        }
        if (!Elevated.handle(input)) InputService.handle(input)
    }

    override fun webcamRequested(pcId: String, options: WebcamOptions): Boolean {
        val request = WebcamRequest(
            pcId = pcId,
            width = options.width.toInt(),
            height = options.height.toInt(),
            fps = options.fps.toInt(),
            bitrate = options.bitrate.toInt(),
            camera = options.camera,
        )
        return onWebcam(pcId, request)
    }

    override fun micRequested(pcId: String): Boolean = onMic(pcId)

    override fun micStopRequested(pcId: String) {
        onMicStop(pcId)
        com.webdevpraveen.linkroid.mic.MicService.stop(context, pcId)
    }

    override fun webcamStopRequested(pcId: String) {
        WebcamRequests.dismiss(context, pcId)
        WebcamService.stopForPc(context, pcId)
    }

    override fun webcamKeyframeRequested(pcId: String) =
        WebcamService.keyframe(pcId)

    override fun cameraCaptureRequested(pcId: String, requestId: String, mode: String): Boolean =
        ContinuityCameraRequests.show(context, pcId, requestId, mode, nameOf(pcId))

    override fun cameraCaptureCancelled(pcId: String, requestId: String) {
        ContinuityCameraRequests.cancelFromPc(context, pcId, requestId)
    }

    override fun phoneApps(): List<PhoneApp> = AppWindows.list(context)

    override fun smsThreads(limit: UInt): List<SmsThread> = sms().threads(limit.toInt())

    override fun smsMessages(thread: String, before: Long?, limit: UInt): List<SmsMessage> =
        sms().messages(thread, before, limit.toInt())

    override fun smsSend(to: List<String>, body: String): Boolean = sms().send(to, body)

    override fun sendSmsWithAttachments(to: List<String>, body: String, attachments: List<SmsAttachment>): Boolean =
        sms().send(to, body, attachments)

    override fun smsPart(id: String): SmsPartData? = sms().part(id)

    override fun setPhoneToggle(id: String, value: PhoneToggleValue): Boolean =
        toggles().set(id, value)

    override fun storageList(path: String): List<StorageEntry> = storage().list(path)

    override fun storageOpenRead(path: String): StorageReadFile = storage().openRead(path)

    override fun storageWrite(path: String, stagedPath: String, modified: Long?): StorageWriteDone =
        storage().write(path, stagedPath, modified)

    override fun storageMkdir(path: String) = storage().mkdir(path)

    override fun storageRename(from: String, to: String) = storage().rename(from, to)

    override fun storageDelete(path: String, confirmed: Boolean) = storage().delete(path, confirmed)

    override fun dismissNotification(key: String) {
        val listener = NotificationListener.instance ?: throw NotificationFailure.Unsupported()
        listener.dismiss(key)
    }

    override fun runNotificationAction(key: String, action: String, reply: String?) {
        val listener = NotificationListener.instance ?: throw NotificationFailure.Unsupported()
        val sbn = listener.find(key) ?: throw NotificationFailure.NotFound()
        val target = action.toIntOrNull()?.let { sbn.notification.actions?.getOrNull(it) }
            ?: throw NotificationFailure.NotFound()
        val intent = target.actionIntent ?: throw NotificationFailure.NotFound()

        // A reply goes to the app the way the notification shade sends it:
        // the text as the result of the action's free-form input.
        val fillIn = Intent()
        if (reply != null) {
            val inputs = target.remoteInputs.orEmpty().filter { it.allowFreeFormInput }
            if (inputs.isEmpty()) throw NotificationFailure.NotFound()
            val results = Bundle().apply { inputs.forEach { putCharSequence(it.resultKey, reply) } }
            RemoteInput.addResultsToIntent(inputs.toTypedArray(), fillIn, results)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                RemoteInput.setResultsSource(fillIn, RemoteInput.SOURCE_FREE_FORM_INPUT)
            }
        }
        try {
            intent.send(context, 0, fillIn, null, null, null, sendOptions())
        } catch (e: PendingIntent.CanceledException) {
            throw NotificationFailure.NotFound()
        }
    }

    /** Lets an action open its app's screen, as tapping it on the phone would. */
    private fun sendOptions(): Bundle? {
        val mode = when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.BAKLAVA -> ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOW_ALWAYS
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE -> {
                @Suppress("DEPRECATION")
                ActivityOptions.MODE_BACKGROUND_ACTIVITY_START_ALLOWED
            }
            else -> return null
        }
        return ActivityOptions.makeBasic().setPendingIntentBackgroundActivityStartMode(mode).toBundle()
    }

    private companion object {
        /** The phone's own screen's mirroring session. */
        const val SCREEN = 0u
    }
}
