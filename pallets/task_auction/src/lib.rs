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
		sp_runtime::traits::Hash,
		traits::{Currency, ExistenceRequirement, ReservableCurrency, WithdrawReasons},
	};

	type BalanceOf<T> =
		<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
		type Currency: ReservableCurrency<Self::AccountId>;

		#[pallet::constant] // put the constant in metadata
		type MinBounty: Get<BalanceOf<Self>>;
		#[pallet::constant] // put the constant in metadata
		type MinDeposit: Get<BalanceOf<Self>>;
		#[pallet::constant] // put the constant in metadata
		type MinBidRatio: Get<u32>;
		#[pallet::constant] // put the constant in metadata
		type MaxBidCount: Get<u32>;
		#[pallet::constant] // put the constant in metadata
		type MaxDataSize: Get<u32>;
	}

	#[derive(Encode, Decode, TypeInfo, Clone, PartialEq)]
	#[scale_info(skip_type_params(T))]
	pub struct Bid<T: Config>(pub T::AccountId, pub BalanceOf<T>);

	#[derive(Encode, Decode, TypeInfo)]
	#[scale_info(skip_type_params(T))]
	pub struct Auction<T: Config> {
		pub employer: T::AccountId,
		pub arbitrator: T::AccountId,
		pub bounty: BalanceOf<T>,
		pub deposit: BalanceOf<T>,
		pub deadline: T::BlockNumber,
		pub data: BoundedVec<u8, T::MaxDataSize>,
	}

	// The pallet's runtime storage items.
	// https://docs.substrate.io/v3/runtime/storage
	#[pallet::storage]
	#[pallet::getter(fn auctions)]
	pub(super) type Auctions<T: Config> = StorageMap<_, Identity, T::Hash, Auction<T>, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn bids)]
	pub(super) type Bids<T: Config> =
		StorageMap<_, Identity, T::Hash, BoundedVec<Bid<T>, T::MaxBidCount>, ValueQuery>;

	// Pallets use events to inform users when important changes are made.
	// https://docs.substrate.io/v3/runtime/events-and-errors
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		Created { auction_id: T::Hash, bounty: BalanceOf<T>, deadline: T::BlockNumber },

		Bid { auction_id: T::Hash, bidder: T::AccountId, price: BalanceOf<T> },
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		DeadlineExpired,
		MinBountyRequired,
		MinDepositRequired,
		MinBidRatioRequired,
		MaxBidCountExceeded,
		AuctionIdNotFound,

		BidderIsEmployer,
		BidderIsArbitrator,
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
			data: BoundedVec<u8, T::MaxDataSize>,
		) -> DispatchResult {
			// input checks
			let employer = ensure_signed(origin)?;
			ensure!(bounty >= T::MinBounty::get(), Error::<T>::MinBountyRequired);
			ensure!(deposit >= T::MinDeposit::get(), Error::<T>::MinDepositRequired);
			ensure!(
				deadline > frame_system::Pallet::<T>::block_number(),
				Error::<T>::DeadlineExpired
			);

			// reserve balance for bounty and deposit
			T::Currency::reserve(&employer, bounty + deposit)?;

			// generate auction id
			let nonce = frame_system::Pallet::<T>::account_nonce(&employer);
			let auction_id = T::Hashing::hash_of(&(employer.clone(), nonce));

			// create and insert new auction
			let auction = Auction::<T> { employer, arbitrator, bounty, deposit, deadline, data };
			Auctions::<T>::insert(&auction_id, auction);

			Self::deposit_event(Event::<T>::Created { auction_id, bounty, deadline });
			Ok(())
		}

		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn bid(
			origin: OriginFor<T>,
			auction_id: T::Hash,
			price: BalanceOf<T>,
		) -> DispatchResult {
			// input checks
			let bidder = ensure_signed(origin)?;
			let auction = Auctions::<T>::get(&auction_id).ok_or(Error::<T>::AuctionIdNotFound)?;
			ensure!(
				auction.deadline >= frame_system::Pallet::<T>::block_number(),
				Error::<T>::DeadlineExpired
			);
			ensure!(bidder != auction.employer, Error::<T>::BidderIsEmployer);
			ensure!(bidder != auction.arbitrator, Error::<T>::BidderIsArbitrator);
			// fetch existing bids vector
			let mut bids = Bids::<T>::get(&auction_id);
			let prev_bid = bids.last().map(|x| x.clone());

			// check if new bid can be inserted
			bids.try_push(Bid(bidder.clone(), price))
				.map_err(|_| Error::<T>::MaxBidCountExceeded)?;

			// if there is a previous bid, ensure new bid is lower,
			// then unreserve deposit of previous bidder
			if let Some(Bid(prev_bidder, prev_price)) = prev_bid {
				ensure!(prev_price > price, Error::<T>::MinBidRatioRequired);
				T::Currency::unreserve(&prev_bidder, auction.deposit);
			}
			// all checks pass, reserve deposit and insert bid
			T::Currency::reserve(&bidder, auction.deposit)?;
			Bids::<T>::insert(&auction_id, bids);

			Self::deposit_event(Event::<T>::Bid { auction_id, bidder, price });
			Ok(())
		}
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);
}
