#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::pallet_prelude::*;
	use frame_system::pallet_prelude::*;

	use frame_support::{
		inherent::Vec,
		sp_runtime::traits::AccountIdConversion,
		traits::{Currency, ExistenceRequirement, WithdrawReasons},
		PalletId,
	};

	const PALLET_ID: frame_support::PalletId = PalletId(*b"task_auc");

	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	#[derive(Encode, Decode, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Bid<T: Config>(T::AccountId, BalanceOf<T>);

	#[derive(Encode, Decode, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Auction<T: Config> {
		pub employer: T::AccountId,
		pub arbitrator: T::AccountId,
		pub bounty: BalanceOf<T>,
		pub deposit: BalanceOf<T>,
		pub deadline: T::BlockNumber,
		pub data: Vec<u8>,
		pub bids: Vec<Bid<T>>,
	}

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Currency: Currency<Self::AccountId>;

		#[pallet::constant] // put the constant in metadata
		type MinBounty: Get<BalanceOf<Self>>;
		#[pallet::constant] // put the constant in metadata
		type MinDeposit: Get<BalanceOf<Self>>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	// The pallet's runtime storage items.
	// https://docs.substrate.io/v3/runtime/storage
	#[pallet::storage]
	#[pallet::getter(fn auction_count)]
	pub(super) type AuctionCount<T: Config> = StorageValue<_, u64, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn auctions)]
	pub(super) type Auctions<T: Config> =
		StorageMap<_, Identity, T::AccountId, Auction<T>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn something)]
	// Learn more about declaring storage items:
	// https://docs.substrate.io/v3/runtime/storage#declaring-storage-items
	pub(super) type Something<T> = StorageValue<_, u32>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// Event documentation should end with an array that provides descriptive names for event
		/// parameters. [something, who]
		SomethingStored(u32, T::AccountId),

		Created {
			auction_id: T::AccountId,
			bounty: BalanceOf<T>,
			deadline: T::BlockNumber,
		},
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Error names should be descriptive.
		NoneValue,

		DeadlineExpired,
		MinBountyRequired,
		MinDepositRequired,

		/// Errors should have helpful documentation associated with them.
		StorageOverflow,
	}

	// Dispatchable functions allows users to interact with the pallet and invoke state changes.
	// These functions materialize as "extrinsics", which are often compared to transactions.
	// Dispatchable functions must be annotated with a weight and must return a DispatchResult.
	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn create(
			origin: OriginFor<T>,
			arbitrator: T::AccountId,
			bounty: BalanceOf<T>,
			deposit: BalanceOf<T>,
			deadline: T::BlockNumber,
			data: Vec<u8>,
		) -> DispatchResult {
			// input checks
			let employer = ensure_signed(origin)?;
			ensure!(bounty >= T::MinBounty::get(), Error::<T>::MinBountyRequired);
			ensure!(deposit >= T::MinDeposit::get(), Error::<T>::MinDepositRequired);
			ensure!(
				deadline > frame_system::Pallet::<T>::block_number(),
				Error::<T>::DeadlineExpired
			);

			// generate auction id
			let auction_count = AuctionCount::<T>::get();
			let auction_id: T::AccountId = PALLET_ID.into_sub_account(auction_count);

			// transfer balances
			let imbalance = T::Currency::withdraw(
				&employer,
				bounty + deposit,
				WithdrawReasons::TRANSFER,
				ExistenceRequirement::AllowDeath,
			)?;
			T::Currency::resolve_creating(&auction_id, imbalance);

			// create new auction
			let auction = Auction::<T> {
				employer,
				arbitrator,
				bounty,
				deposit,
				deadline,
				data,
				bids: Vec::new(),
			};

            // update storage
			Auctions::<T>::insert(&auction_id, auction);
			AuctionCount::<T>::put(auction_count + 1);

			// broadcast event and finalize
			Self::deposit_event(Event::<T>::Created { auction_id, bounty, deadline });
			Ok(())
		}

		/// An example dispatchable that takes a singles value as a parameter, writes the value to
		/// storage and emits an event. This function must be dispatched by a signed extrinsic.
		#[pallet::weight(10_000 + T::DbWeight::get().writes(1))]
		pub fn do_something(origin: OriginFor<T>, something: u32) -> DispatchResult {
			// Check that the extrinsic was signed and get the signer.
			// This function will return an error if the extrinsic is not signed.
			// https://docs.substrate.io/v3/runtime/origins
			let who = ensure_signed(origin)?;

			// Update storage.
			<Something<T>>::put(something);

			// Emit an event.
			Self::deposit_event(Event::SomethingStored(something, who));
			// Return a successful DispatchResultWithPostInfo
			Ok(())
		}

		/// An example dispatchable that may throw a custom error.
		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn cause_error(origin: OriginFor<T>) -> DispatchResult {
			let _who = ensure_signed(origin)?;

			// Read a value from storage.
			match <Something<T>>::get() {
				// Return an error if the value has not been set.
				None => Err(Error::<T>::NoneValue)?,
				Some(old) => {
					// Increment the value read from storage; will error in the event of overflow.
					let new = old.checked_add(1).ok_or(Error::<T>::StorageOverflow)?;
					// Update the value in storage with the incremented result.
					<Something<T>>::put(new);
					Ok(())
				},
			}
		}
	}
}
