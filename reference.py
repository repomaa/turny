import spotipy
import RPi.GPIO as GPIO
import subprocess
import time
import threading
import logging
from spotipy.oauth2 import SpotifyOAuth
from gpiozero import Button, LED
from mfrc522 import SimpleMFRC522

logging.basicConfig(
    level=logging.INFO,
    format='[%(levelname)s] - %(message)s',
    handlers=[logging.StreamHandler()]
)
logger = logging.getLogger(__name__)

GPIO.setwarnings(False)

class Turny:
  def __init__(self):
    scope = 'user-read-playback-state user-modify-playback-state'
    self.button = Button(27)
    self.led = LED(22)
    self.reader = SimpleMFRC522()
    self.device_id = 'd295ff8dc55fa0b2ec7f612119675301d38f802c'
    auth_manager = SpotifyOAuth(
      scope=scope,
      client_id='6408760457ed45538740a3f13f369722',
      client_secret='72ad08a2fe204c8894bdb1a7a8c9a866',
      redirect_uri='https://jokke.space/callback',
      open_browser=False,
    )

    self.sp = spotipy.Spotify(auth_manager=auth_manager)

    self.playlist_map = {
            '383951559086': 'spotify:playlist:4Y6ZFtrQX7vuKVGLbNQ5sN',
    }

    self.id = None
    self.context_uri = None
    self.is_playing = False
    self.last_heartbeat = time.time()
    self.heartbeat_interval = 30  # Check every 30 seconds
    self.button_press_start = None
    self.button_action_handled = False

    # Button event handlers
    self.button.when_pressed = self.handle_button_press
    self.button.when_released = self.handle_button_release

  def read_id(self):
    return self.reader.read_id_no_block()

  def reset_id(self):
    self.id = None
    self.context_uri = None

  def handle_button_press(self):
    self.button_press_start = time.time()
    self.button_action_handled = False

  def handle_button_release(self):
    if self.button_press_start is None or self.button_action_handled:
      return

    press_duration = time.time() - self.button_press_start

    if press_duration >= 5.0:
      # Manual reset - already handled in run loop
      pass
    elif press_duration >= 1.0:
      # Previous track
      try:
        logger.info("Previous track")
        self.sp.previous_track(device_id=self.device_id)
      except Exception as e:
        logger.error(f"Error going to previous track: {e}")
    else:
      # Next track
      try:
        logger.info("Next track")
        self.sp.next_track(device_id=self.device_id)
      except Exception as e:
        logger.error(f"Error going to next track: {e}")

    self.button_press_start = None

  def check_heartbeat(self):
    """Check if the Spotify device is still available"""
    retries = 0
    while retries < 10:
      time_to_retry = 1 + retries * 0.5
      try:
        devices = self.sp.devices()
        for device in devices['devices']:
          if device['id'] == self.device_id and device['is_active']:
            return True

        logger.error(f"Heartbeat check ({retries}/10) failed: device missing or inactive. Next retry in {time_to_retry}s")
      except Exception as e:
        logger.error(f"Heartbeat check ({retries + 1}/10) failed: {e}. Next retry in {time_to_retry}s")
      retries += 1
      time.sleep(time_to_retry)

    return False

  def restart_spotifyd(self):
    """Restart the spotifyd service"""
    try:
      logger.info("Restarting spotifyd...")
      subprocess.run(['systemctl', '--user', 'restart', 'spotifyd'], check=True)
      time.sleep(5)  # Wait for service to restart
      logger.info("Spotifyd restarted")
      return True
    except subprocess.CalledProcessError as e:
      logger.error(f"Failed to restart spotifyd: {e}")
      return False

  def manual_reset(self):
    """Perform a manual reset of the system"""
    logger.info("Manual reset triggered")

    # Visual confirmation - blink rapidly
    self.led.blink(on_time=0.1, off_time=0.1, n=10, background=False)

    # Reset internal state
    self.reset_id()
    self.is_playing = False

    # Try to pause any current playback
    try:
      self.sp.pause_playback(device_id=self.device_id)
    except Exception as e:
      logger.error(f"Error pausing during reset: {e}")

    # Restart spotifyd
    if self.restart_spotifyd():
      # Success confirmation - slow blink
      self.led.blink(on_time=0.5, off_time=0.5, n=3, background=False)
    else:
      # Error confirmation - fast blink
      self.led.blink(on_time=0.05, off_time=0.05, n=20, background=False)

    self.led.off()

  def run(self):
    try:
      self.sp.pause_playback(device_id=self.device_id)
      self.sp.repeat('context', device_id=self.device_id)
      self.sp.volume(70, device_id=self.device_id)
    except Exception as e:
      logger.error(f"Error setting initial state: {e}")

    time.sleep(1)
    self.led.blink(on_time=0.5, off_time=0.5, n=3, background=True)
    subprocess.run(['aplay', 'startup.wav'])
    absence_count = 0

    try:
      while True:
        # Check for manual reset (5+ second button press)
        if (self.button_press_start is not None and
            not self.button_action_handled and
            time.time() - self.button_press_start >= 5.0):
          self.button_action_handled = True
          self.manual_reset()
          continue

        # Periodic heartbeat check
        if time.time() - self.last_heartbeat >= self.heartbeat_interval:
          self.last_heartbeat = time.time()
          if not self.check_heartbeat():
            logger.error("Heartbeat failed, attempting to restart spotifyd...")
            if self.restart_spotifyd():
              # Reset state after restart
              self.reset_id()
              self.is_playing = False
              self.led.off()
            else:
              logger.error("Failed to restart spotifyd, continuing...")

        # RFID reading logic
        id = self.read_id()

        if id:
          absence_count = 0

          if id != self.id:
            self.id = id
            self.context_uri = self.playlist_map.get(str(id))

            # Reset playing state when chip changes
            self.is_playing = False
            logger.info(f"New chip detected: {id}")

          # Start playback if we have a valid playlist and aren't already playing
          if self.context_uri and not self.is_playing:
            try:
              self.led.on()
              logger.info('Starting playback of: ' + self.context_uri)
              self.sp.start_playback(device_id=self.device_id,
                                   context_uri=self.context_uri,
                                   offset={'position': 0})
              self.is_playing = True
            except Exception as e:
              logger.error(f"Error starting playback: {e}")
              self.led.off()
              self.is_playing = False
          elif not self.context_uri:
            logger.warning(f"Unknown chip: {id}")

        else:
          absence_count += 1
          if absence_count > 3 and self.is_playing:
            try:
              self.led.off()
              logger.info('Pausing playback')
              self.sp.pause_playback(device_id=self.device_id)
              self.is_playing = False
            except Exception as e:
              logger.error(f"Error pausing playback: {e}")
              # Still set is_playing to False to prevent stuck state
              self.is_playing = False

        time.sleep(0.05)
    finally:
      GPIO.cleanup()

turny = Turny()
turny.run()
